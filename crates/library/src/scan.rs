//! The scanner thread: walks the library folders, stores what changed, then
//! fills in audio hashes in the background. A file watcher triggers rescans.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant, UNIX_EPOCH},
};

use notify::{RecursiveMode, Watcher};
use rusqlite::{Connection, OptionalExtension, params};
use walkdir::WalkDir;

use crate::{Shared, db, hash, tags};

pub enum Request {
    /// Scan every root now.
    Rescan,
    /// Something changed on disk; rescan once things settle.
    Changed,
    /// The set of roots changed: rewatch and rescan.
    Roots,
}

/// Waits this long after the last file change before rescanning, so copying
/// an album triggers one scan, not one per file.
const SETTLE: Duration = Duration::from_secs(3);

pub fn run(
    shared: Arc<Shared>,
    mut conn: Connection,
    requests: mpsc::Receiver<Request>,
    notify_tx: mpsc::Sender<Request>,
) {
    let mut watcher = make_watcher(notify_tx);
    rewatch(&mut watcher, &conn);
    let mut pending = true; // scan at start-up
    let mut changed_at: Option<Instant> = None;
    loop {
        if pending {
            pending = false;
            if let Err(err) = scan(&shared, &mut conn) {
                tracing::error!(%err, "library scan failed");
                shared.update(|s| s.error = Some(err.to_string()));
            }
        }
        // Hash in the background, one file at a time, giving way to scans.
        let request = match changed_at {
            Some(at) => requests.recv_timeout(SETTLE.saturating_sub(at.elapsed())),
            None if hash_next(&shared, &conn) => match requests.try_recv() {
                Ok(r) => Ok(r),
                Err(mpsc::TryRecvError::Empty) => continue,
                Err(mpsc::TryRecvError::Disconnected) => return,
            },
            None => requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match request {
            Ok(Request::Rescan) => pending = true,
            Ok(Request::Changed) => changed_at = Some(Instant::now()),
            Ok(Request::Roots) => {
                rewatch(&mut watcher, &conn);
                pending = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                changed_at = None;
                pending = true;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn make_watcher(tx: mpsc::Sender<Request>) -> Option<notify::RecommendedWatcher> {
    notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| !e.kind.is_access()) {
            let _ = tx.send(Request::Changed);
        }
    })
    .inspect_err(|err| tracing::warn!(%err, "can't watch library folders"))
    .ok()
}

fn rewatch(watcher: &mut Option<notify::RecommendedWatcher>, conn: &Connection) {
    let Some(watcher) = watcher else { return };
    // notify has no "unwatch all"; recreating per root change is overkill, so
    // just add every root (adding twice is harmless).
    for root in db::roots(conn).unwrap_or_default() {
        if let Err(err) = watcher.watch(Path::new(&root), RecursiveMode::Recursive) {
            tracing::warn!(root, %err, "can't watch folder");
        }
    }
}

fn mtime(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Brings the database in line with the folders: new and changed files are
/// read, files that are gone are removed. A root that is missing (an
/// unplugged drive) is left alone rather than emptied.
fn scan(shared: &Shared, conn: &mut Connection) -> rusqlite::Result<()> {
    let roots = db::roots(conn)?;
    shared.update(|s| {
        s.scanning = true;
        s.scanned = 0;
        s.error = None;
    });
    let started = Instant::now();
    let mut folder_art = tags::FolderArt::default();
    let mut missing = Vec::new();
    let mut changes = 0;
    for root in &roots {
        if !Path::new(root).is_dir() {
            missing.push(root.clone());
            continue;
        }
        let known: std::collections::HashMap<String, (i64, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT path, size, mtime FROM tracks WHERE substr(path, 1, length(?1)) = ?1",
            )?;
            stmt.query_map([root], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
                .collect::<rusqlite::Result<_>>()?
        };
        let mut seen = HashSet::new();
        let mut batch = conn.transaction()?;
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if !entry.file_type().is_file() || !tags::is_audio(path) {
                continue;
            }
            let Some(path_str) = path.to_str().map(str::to_string) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            let (size, mtime) = (meta.len() as i64, mtime(&meta));
            seen.insert(path_str.clone());
            shared.update(|s| s.scanned += 1);
            if known.get(&path_str) == Some(&(size, mtime)) {
                continue;
            }
            store(
                &batch,
                shared,
                path,
                &path_str,
                size,
                mtime,
                &mut folder_art,
            )?;
            changes += 1;
            // Commit in batches so others see progress and a crash loses little.
            if changes % 200 == 0 {
                batch.commit()?;
                let tracks = db::track_count(conn)?;
                shared.update(|s| {
                    s.revision += 1;
                    s.tracks = tracks;
                });
                batch = conn.transaction()?;
            }
        }
        for gone in known.keys().filter(|p| !seen.contains(*p)) {
            batch.execute("DELETE FROM tracks WHERE path = ?1", [gone])?;
            changes += 1;
        }
        batch.commit()?;
    }
    conn.execute(
        "DELETE FROM art WHERE key NOT IN (SELECT art FROM tracks WHERE art IS NOT NULL)",
        [],
    )?;
    let tracks = db::track_count(conn)?;
    let hashing = conn.query_row(
        "SELECT count(*) FROM tracks WHERE audio_hash IS NULL",
        [],
        |r| r.get(0),
    )?;
    tracing::info!(tracks, changes, elapsed = ?started.elapsed(), "library scan done");
    shared.update(|s| {
        s.scanning = false;
        s.tracks = tracks;
        s.hashing = hashing;
        s.roots = roots.clone();
        if changes > 0 {
            s.revision += 1;
        }
        if !missing.is_empty() {
            s.error = Some(format!("not available: {}", missing.join(", ")));
        }
    });
    Ok(())
}

fn store(
    tx: &rusqlite::Transaction<'_>,
    shared: &Shared,
    path: &Path,
    path_str: &str,
    size: i64,
    mtime: i64,
    folder_art: &mut tags::FolderArt,
) -> rusqlite::Result<()> {
    let t = tags::read(path);
    let art = match &t.picture {
        Some((bytes, ext)) => save_embedded_art(shared, tx, bytes, ext),
        None => folder_art.find(path).and_then(|file| {
            let bytes = fs::read(&file).ok()?;
            let key = art_key(&bytes);
            tx.execute(
                "INSERT OR IGNORE INTO art (key, path) VALUES (?1, ?2)",
                params![key, file.to_str()?],
            )
            .ok()?;
            Some(key)
        }),
    };
    // A changed file keeps its row (and id) but loses a hash that may be stale.
    tx.execute(
        "INSERT INTO tracks (path, size, mtime, title, artist, album, album_artist, track_no, disc_no, year, genre,
             duration_ms, codec, sample_rate, bit_depth, channels, bitrate, musicbrainz_recording, isrc, art)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
         ON CONFLICT (path) DO UPDATE SET size = ?2, mtime = ?3, title = ?4, artist = ?5, album = ?6,
             album_artist = ?7, track_no = ?8, disc_no = ?9, year = ?10, genre = ?11, duration_ms = ?12,
             codec = ?13, sample_rate = ?14, bit_depth = ?15, channels = ?16, bitrate = ?17,
             musicbrainz_recording = ?18, isrc = ?19, art = ?20, audio_hash = NULL",
        params![
            path_str,
            size,
            mtime,
            t.title,
            t.artist,
            t.album,
            t.album_artist,
            t.track_no,
            t.disc_no,
            t.year,
            t.genre,
            t.duration_ms.map(|d| d as i64),
            t.codec,
            t.sample_rate,
            t.bit_depth,
            t.channels,
            t.bitrate,
            t.musicbrainz_recording,
            t.isrc,
            art,
        ],
    )?;
    Ok(())
}

pub fn art_key(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_string()
}

/// Copies an embedded picture into the artwork cache once per distinct image.
fn save_embedded_art(
    shared: &Shared,
    tx: &rusqlite::Transaction<'_>,
    bytes: &[u8],
    ext: &str,
) -> Option<String> {
    let key = art_key(bytes);
    let file = shared.art_dir.join(format!("{key}.{ext}"));
    if !file.exists() {
        fs::write(&file, bytes)
            .inspect_err(|err| tracing::warn!(%err, "can't cache artwork"))
            .ok()?;
    }
    tx.execute(
        "INSERT OR IGNORE INTO art (key, path) VALUES (?1, ?2)",
        params![key, file.to_str()?],
    )
    .ok()?;
    Some(key)
}

/// Hashes one track still missing its audio hash. Returns false when there
/// is nothing left to do.
fn hash_next(shared: &Shared, conn: &Connection) -> bool {
    let next: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, path FROM tracks WHERE audio_hash IS NULL ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((id, path)) = next else { return false };
    // Unreadable files get an empty hash so they aren't retried every time.
    let value = hash::audio_hash(&PathBuf::from(&path)).unwrap_or_else(|err| {
        tracing::debug!(path, %err, "can't hash");
        String::new()
    });
    let _ = conn.execute(
        "UPDATE tracks SET audio_hash = ?1 WHERE id = ?2",
        params![value, id],
    );
    shared.update(|s| s.hashing = s.hashing.saturating_sub(1));
    true
}
