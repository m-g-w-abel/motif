//! The scanner thread: walks the library folders, stores what changed, then
//! fills in audio hashes in the background. A file watcher (optional)
//! triggers rescans when files change outside Motif.
//!
//! The database is the source of truth for the library: a file that
//! disappears is marked missing, never forgotten, so it can be found or
//! fetched again later. Only removing a folder in Motif deletes tracks.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
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
    /// The roots or the watch setting changed: rewatch and rescan.
    Roots,
}

/// Waits this long after the last file change before rescanning, so copying
/// an album triggers one scan, not one per file.
const SETTLE: Duration = Duration::from_secs(3);

/// Scan progress is published every this many files, not every file, so a
/// fast disk doesn't flood every screen with updates.
const PROGRESS_STEP: u32 = 100;

pub fn run(
    shared: Arc<Shared>,
    mut conn: Connection,
    requests: mpsc::Receiver<Request>,
    notify_tx: mpsc::Sender<Request>,
) {
    let mut watcher = None;
    rewatch(&mut watcher, &notify_tx, &conn);
    let mut pending = true; // scan at start-up
    let mut changed_at: Option<Instant> = None;
    let mut thumbs = Vec::new();
    loop {
        if pending {
            pending = false;
            if let Err(err) = scan(&shared, &mut conn) {
                tracing::error!(%err, "library scan failed");
                shared.update(|s| s.error = Some(err.to_string()));
            }
            thumbs = missing_bar_thumbs(&shared, &conn);
        }
        // Make bar thumbnails, then hash, in the background, one file at a
        // time, giving way to scans.
        let request = match changed_at {
            Some(at) => requests.recv_timeout(SETTLE.saturating_sub(at.elapsed())),
            None if thumb_next(&shared, &mut thumbs) || hash_next(&shared, &conn) => {
                match requests.try_recv() {
                    Ok(r) => Ok(r),
                    Err(mpsc::TryRecvError::Empty) => continue,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            None => requests
                .recv()
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match request {
            Ok(Request::Rescan) => pending = true,
            Ok(Request::Changed) => changed_at = Some(Instant::now()),
            Ok(Request::Roots) => {
                rewatch(&mut watcher, &notify_tx, &conn);
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

/// Watches every root for outside changes, or nothing if watching is off.
fn rewatch(
    watcher: &mut Option<notify::RecommendedWatcher>,
    tx: &mpsc::Sender<Request>,
    conn: &Connection,
) {
    // Recreated each time: notify has no "unwatch everything".
    *watcher = None;
    if !db::watching(conn) {
        return;
    }
    let tx = tx.clone();
    let made = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok_and(|e| !e.kind.is_access()) {
            let _ = tx.send(Request::Changed);
        }
    });
    let Ok(mut new) = made.inspect_err(|err| tracing::warn!(%err, "can't watch library folders"))
    else {
        return;
    };
    for root in db::roots(conn).unwrap_or_default() {
        if let Err(err) = new.watch(Path::new(&root), RecursiveMode::Recursive) {
            tracing::warn!(root, %err, "can't watch folder");
        }
    }
    *watcher = Some(new);
}

fn mtime(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// What the database knows about a file under a root.
struct Known {
    size: i64,
    mtime: i64,
    missing: bool,
}

/// Brings the database in line with the folders: new and changed files are
/// read, files that are gone are marked missing (and unmarked if they come
/// back). A root that is unavailable (unplugged, or an empty mount point
/// where tracks used to be) is left untouched.
fn scan(shared: &Shared, conn: &mut Connection) -> rusqlite::Result<()> {
    let roots = db::roots(conn)?;
    shared.update(|s| {
        s.scanning = true;
        s.scanned = 0;
        s.error = None;
    });
    let started = Instant::now();
    let mut folder_art = tags::FolderArt::default();
    let mut unavailable = Vec::new();
    let mut changes = 0;
    let mut scanned = 0u32;
    for root in &roots {
        let known: HashMap<String, Known> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT path, size, mtime, missing FROM tracks WHERE {}",
                db::UNDER_ROOT
            ))?;
            stmt.query_map([root], |r| {
                Ok((
                    r.get(0)?,
                    Known {
                        size: r.get(1)?,
                        mtime: r.get(2)?,
                        missing: r.get(3)?,
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        let files: Vec<_> = if Path::new(root).is_dir() {
            WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && tags::is_audio(e.path()))
                .collect()
        } else {
            Vec::new()
        };
        if files.is_empty() && !known.is_empty() {
            // Looks unplugged: marking everything missing would only be noise.
            unavailable.push(root.clone());
            continue;
        }

        let mut seen = HashSet::new();
        let mut batch = conn.transaction()?;
        for entry in files {
            let path = entry.path();
            let Some(path_str) = path.to_str().map(str::to_string) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            let (size, mtime) = (meta.len() as i64, mtime(&meta));
            seen.insert(path_str.clone());
            scanned += 1;
            if scanned.is_multiple_of(PROGRESS_STEP) {
                shared.update(|s| s.scanned = scanned);
            }
            match known.get(&path_str) {
                Some(k) if (k.size, k.mtime) == (size, mtime) => {
                    if k.missing {
                        batch.execute(
                            "UPDATE tracks SET missing = 0 WHERE path = ?1",
                            [&path_str],
                        )?;
                        changes += 1;
                    }
                    continue;
                }
                _ => {}
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
        for (path, _) in known
            .iter()
            .filter(|(p, k)| !k.missing && !seen.contains(*p))
        {
            batch.execute("UPDATE tracks SET missing = 1 WHERE path = ?1", [path])?;
            changes += 1;
        }
        batch.commit()?;
    }
    let tracks = db::track_count(conn)?;
    let missing = conn.query_row("SELECT count(*) FROM tracks WHERE missing = 1", [], |r| {
        r.get(0)
    })?;
    let hashing = conn.query_row(
        "SELECT count(*) FROM tracks WHERE audio_hash IS NULL AND missing = 0",
        [],
        |r| r.get(0),
    )?;
    tracing::info!(tracks, changes, elapsed = ?started.elapsed(), "library scan done");
    shared.update(|s| {
        s.scanning = false;
        s.scanned = scanned;
        s.tracks = tracks;
        s.missing = missing;
        s.hashing = hashing;
        s.roots = roots.clone();
        if changes > 0 {
            s.revision += 1;
        }
        if !unavailable.is_empty() {
            s.error = Some(format!("not available: {}", unavailable.join(", ")));
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
    let folder = folder_art.find(path);
    let art = match (&t.picture, folder) {
        (Some((bytes, ext)), folder) if embedded_wins(bytes, folder.as_ref()) => {
            save_embedded_art(shared, tx, bytes, ext)
        }
        (_, Some(image)) => image
            .path
            .to_str()
            .and_then(|path| {
                tx.execute(
                    "INSERT OR IGNORE INTO art (key, path) VALUES (?1, ?2)",
                    params![image.key, path],
                )
                .ok()
            })
            .map(|_| image.key),
        // An embedded picture without a folder image always wins above.
        (_, None) => None,
    };
    // A changed file keeps its row (and id) but loses a hash that may be
    // stale and any problem noted before.
    tx.execute(
        "INSERT INTO tracks (path, size, mtime, title, artist, album, album_artist, track_no, disc_no, year, genre,
             duration_ms, codec, sample_rate, bit_depth, channels, bitrate, musicbrainz_recording, isrc, art,
             rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, added, musicbrainz_album)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
             ?21, ?22, ?23, ?24, CAST(strftime('%s', 'now') AS INTEGER), ?25)
         ON CONFLICT (path) DO UPDATE SET size = ?2, mtime = ?3, title = ?4, artist = ?5, album = ?6,
             album_artist = ?7, track_no = ?8, disc_no = ?9, year = ?10, genre = ?11, duration_ms = ?12,
             codec = ?13, sample_rate = ?14, bit_depth = ?15, channels = ?16, bitrate = ?17,
             musicbrainz_recording = ?18, isrc = ?19, art = ?20, rg_track_gain = ?21, rg_track_peak = ?22,
             rg_album_gain = ?23, rg_album_peak = ?24, musicbrainz_album = ?25, audio_hash = NULL, missing = 0, problem = NULL",
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
            t.replay_gain.track_gain,
            t.replay_gain.track_peak,
            t.replay_gain.album_gain,
            t.replay_gain.album_peak,
            t.musicbrainz_album,
        ],
    )?;
    Ok(())
}

/// Whether a track shows its embedded picture rather than the folder image:
/// the folder image only when it is larger by pixel area, so albums with a
/// cover per track keep them (#18).
fn embedded_wins(embedded: &[u8], folder: Option<&tags::FolderImage>) -> bool {
    folder.is_none_or(|f| tags::pixels(embedded) >= f.pixels)
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
    let (key, file) = cache_picture(shared, bytes, ext)?;
    tx.execute(
        "INSERT OR IGNORE INTO art (key, path) VALUES (?1, ?2)",
        params![key, file.to_str()?],
    )
    .ok()?;
    Some(key)
}

/// Writes an embedded picture to the artwork folder (once), returning its
/// key and file.
pub(crate) fn cache_picture(
    shared: &Shared,
    bytes: &[u8],
    ext: &str,
) -> Option<(String, std::path::PathBuf)> {
    let key = art_key(bytes);
    let file = shared.art_dir.join(format!("{key}.{ext}"));
    if !file.exists() {
        fs::write(&file, bytes)
            .inspect_err(|err| tracing::warn!(%err, "can't cache artwork"))
            .ok()?;
    }
    Some((key, file))
}

/// Artwork (key, path) without a player bar thumbnail yet.
fn missing_bar_thumbs(shared: &Shared, conn: &Connection) -> Vec<(String, String)> {
    let all = conn
        .prepare("SELECT key, path FROM art")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<(String, String)>>>()
        });
    all.unwrap_or_default()
        .into_iter()
        .filter(|(key, _)| !crate::thumb_path(shared, key, crate::BAR_ART_SIZE).exists())
        .collect()
}

/// Makes one bar thumbnail from `thumbs`. Returns false when there are none left.
fn thumb_next(shared: &Shared, thumbs: &mut Vec<(String, String)>) -> bool {
    let Some((key, path)) = thumbs.pop() else {
        return false;
    };
    if let Err(err) = crate::thumbnail(shared, &key, Path::new(&path), crate::BAR_ART_SIZE) {
        tracing::debug!(key, %err, "can't make bar thumbnail");
    }
    true
}

/// Hashes one track still missing its audio hash. Returns false when there
/// is nothing left to do, or when a file has vanished (its drive was
/// probably unplugged): hashing then waits for the next scan to sort out
/// what is really missing.
fn hash_next(shared: &Shared, conn: &Connection) -> bool {
    let next: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, path FROM tracks WHERE audio_hash IS NULL AND missing = 0 ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((id, path)) = next else { return false };
    let result = match hash::audio_hash(Path::new(&path)) {
        Ok(hash) => conn.execute(
            "UPDATE tracks SET audio_hash = ?1 WHERE id = ?2",
            params![hash, id],
        ),
        Err(_) if !Path::new(&path).exists() => return false,
        // Present but undecodable: note why, with an empty hash so it isn't retried.
        Err(problem) => {
            tracing::debug!(path, problem, "damaged file");
            conn.execute(
                "UPDATE tracks SET audio_hash = '', problem = ?1 WHERE id = ?2",
                params![problem, id],
            )
        }
    };
    if let Err(err) = result {
        tracing::warn!(%err, "can't store audio hash");
        return false;
    }
    shared.update(|s| s.hashing = s.hashing.saturating_sub(1));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(side: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbImage::new(side, side)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    /// Whether the embedded picture (if `embedded` is a side) beats `cover.png`
    /// (if `folder` is a side), found as the scanner finds it.
    fn embedded_shows(embedded: Option<u32>, folder: Option<u32>) -> bool {
        let dir = std::env::temp_dir().join(format!(
            "motif-art-{}-{embedded:?}-{folder:?}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        if let Some(side) = folder {
            fs::write(dir.join("cover.png"), png(side)).unwrap();
        }
        let image = tags::FolderArt::default().find(&dir.join("track.flac"));
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(image.is_some(), folder.is_some());
        embedded.is_some_and(|side| embedded_wins(&png(side), image.as_ref()))
    }

    #[test]
    fn the_larger_art_wins_and_a_tie_goes_to_the_embedded() {
        assert!(embedded_shows(Some(600), Some(300)), "embedded larger");
        assert!(!embedded_shows(Some(300), Some(600)), "folder larger");
        assert!(embedded_shows(Some(300), Some(300)), "tie");
        assert!(!embedded_shows(None, Some(300)), "folder only");
        assert!(embedded_shows(Some(300), None), "embedded only");
    }
}
