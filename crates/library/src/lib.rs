//! The music library: what files this device has, their tags, artwork and
//! identity keys, in a SQLite database. It knows nothing about playback.
//!
//! A scanner thread keeps the database in line with the library folders
//! (watching them for changes) and fills in audio hashes in the background.

mod db;
mod hash;
mod scan;
mod tags;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

use protocol::{
    Command, ContextKind, LibraryAlbum, LibraryQuery, LibraryStatus, LibraryTrack, QueryResult,
    SortRule, TrackGroup, TrackKeys, TrackRef,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use tokio::sync::watch;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// State shared between the handle and the scanner thread.
struct Shared {
    status: watch::Sender<LibraryStatus>,
    art_dir: PathBuf,
}

impl Shared {
    fn update(&self, f: impl FnOnce(&mut LibraryStatus)) {
        self.status.send_modify(f);
    }
}

/// Handle to the library. Cheap to clone.
#[derive(Clone)]
pub struct Library {
    conn: Arc<Mutex<Connection>>,
    shared: Arc<Shared>,
    scanner: mpsc::Sender<scan::Request>,
}

impl Library {
    /// Opens (creating if needed) the library in `data_dir` and starts the
    /// scanner thread, which scans all folders once at start-up.
    pub fn open(data_dir: &Path) -> Result<Self> {
        let art_dir = data_dir.join("art");
        fs::create_dir_all(&art_dir)?;
        let db_path = data_dir.join("library.db");
        let conn = db::open(&db_path)?;
        let status = LibraryStatus {
            roots: db::roots(&conn)?,
            tracks: db::track_count(&conn)?,
            missing: conn.query_row("SELECT count(*) FROM tracks WHERE missing = 1", [], |r| {
                r.get(0)
            })?,
            watching: db::watching(&conn),
            ..Default::default()
        };
        let shared = Arc::new(Shared {
            status: watch::channel(status).0,
            art_dir,
        });
        let (scanner, requests) = mpsc::channel();
        let scanner_conn = db::open(&db_path)?;
        let (thread_shared, notify_tx) = (shared.clone(), scanner.clone());
        std::thread::Builder::new()
            .name("library-scan".into())
            .spawn(move || scan::run(thread_shared, scanner_conn, requests, notify_tx))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            shared,
            scanner,
        })
    }

    pub fn subscribe(&self) -> watch::Receiver<LibraryStatus> {
        self.shared.status.subscribe()
    }

    pub fn status(&self) -> LibraryStatus {
        self.shared.status.borrow().clone()
    }

    fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Answers a query. `Play` and `PlayAlbum` are answered by
    /// [`Self::selection`] instead, since they act on the engine.
    pub fn query(&self, query: LibraryQuery) -> Result<QueryResult> {
        let conn = self.db();
        Ok(match query {
            LibraryQuery::Status => QueryResult::Status {
                status: self.status(),
            },
            LibraryQuery::Tracks {
                search,
                sort,
                offset,
                limit,
            } => {
                let fts = db::fts_query(search.as_deref());
                let total = conn.query_row(
                    &format!("SELECT count(*) FROM tracks WHERE {}", db::SEARCH),
                    [&fts],
                    |r| r.get(0),
                )?;
                let mut stmt = conn.prepare(&format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} {} LIMIT ?2 OFFSET ?3",
                    db::SEARCH,
                    db::order(&sort)
                ))?;
                let tracks = stmt
                    .query_map(params![fts, limit, offset], library_track)?
                    .collect::<rusqlite::Result<_>>()?;
                QueryResult::Tracks {
                    total,
                    offset,
                    tracks,
                }
            }
            LibraryQuery::Groups { search, sort } => QueryResult::Groups {
                groups: groups(&conn, search.as_deref(), &sort)?,
            },
            LibraryQuery::Albums {
                search,
                sort,
                offset,
                limit,
            } => {
                let fts = db::fts_query(search.as_deref());
                let total = conn.query_row(
                    &format!(
                        "SELECT count(DISTINCT {}) FROM tracks WHERE {}",
                        db::ALBUM_KEY,
                        db::SEARCH
                    ),
                    [&fts],
                    |r| r.get(0),
                )?;
                let mut stmt = conn.prepare(&format!(
                    "SELECT {key} AS k, coalesce(album, ''), coalesce(album_artist, artist, ''), min(year),
                            count(*), sum(coalesce(duration_ms, 0)), max(art)
                     FROM tracks WHERE {search} GROUP BY k {order} LIMIT ?2 OFFSET ?3",
                    key = db::ALBUM_KEY,
                    search = db::SEARCH,
                    order = db::album_order(&sort)
                ))?;
                let albums = stmt
                    .query_map(params![fts, limit, offset], |r| {
                        Ok(LibraryAlbum {
                            key: r.get(0)?,
                            title: r.get(1)?,
                            artist: r.get(2)?,
                            year: r.get(3)?,
                            tracks: r.get(4)?,
                            duration_ms: r.get::<_, i64>(5)? as u64,
                            art: r.get(6)?,
                        })
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                QueryResult::Albums {
                    total,
                    offset,
                    albums,
                }
            }
            LibraryQuery::AlbumPosition {
                search,
                sort,
                album,
            } => QueryResult::Position {
                index: groups(&conn, search.as_deref(), &sort)?
                    .into_iter()
                    .find(|g| g.key == album)
                    .map(|g| g.start),
            },
            LibraryQuery::Lookup { paths } => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE path = ?1"
                ))?;
                let mut tracks = Vec::new();
                for path in paths.iter().take(1000) {
                    if let Some(track) = stmt.query_row([path], library_track).optional()? {
                        tracks.push(track);
                    }
                }
                QueryResult::Tracks {
                    total: tracks.len() as u32,
                    offset: 0,
                    tracks,
                }
            }
            LibraryQuery::AlbumTracks { albums, sort } => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} = ?1 {}",
                    db::ALBUM_KEY,
                    db::order(&sort)
                ))?;
                let mut tracks = Vec::new();
                for album in &albums {
                    for track in stmt.query_map([album], library_track)? {
                        tracks.push(track?);
                    }
                }
                QueryResult::Tracks {
                    total: tracks.len() as u32,
                    offset: 0,
                    tracks,
                }
            }
            LibraryQuery::Play { .. } | LibraryQuery::PlayAlbum { .. } => {
                return Err(Error::Other("play queries go through selection()".into()));
            }
            LibraryQuery::AddRoot { path } => {
                let path =
                    fs::canonicalize(&path).map_err(|e| Error::Other(format!("{path}: {e}")))?;
                if !path.is_dir() {
                    return Err(Error::Other(format!("{} is not a folder", path.display())));
                }
                let path = path
                    .to_str()
                    .ok_or_else(|| Error::Other("folder name is not UTF-8".into()))?;
                conn.execute("INSERT OR IGNORE INTO roots (path) VALUES (?1)", [path])?;
                self.roots_changed(&conn)?;
                QueryResult::Done
            }
            LibraryQuery::RemoveRoot { path } => {
                // Roots are stored resolved (AddRoot canonicalizes), so resolve this
                // one too; a folder that no longer exists is matched as given.
                let path = fs::canonicalize(&path)
                    .ok()
                    .and_then(|p| p.to_str().map(str::to_string))
                    .unwrap_or(path);
                // Removing a folder in Motif is the one way tracks leave the library.
                conn.execute("DELETE FROM roots WHERE path = ?1", [&path])?;
                conn.execute(
                    &format!("DELETE FROM tracks WHERE {}", db::UNDER_ROOT),
                    [&path],
                )?;
                self.roots_changed(&conn)?;
                QueryResult::Done
            }
            LibraryQuery::Rescan => {
                let _ = self.scanner.send(scan::Request::Rescan);
                QueryResult::Done
            }
            LibraryQuery::SetWatching { enabled } => {
                db::set_watching(&conn, enabled)?;
                self.shared.update(|s| s.watching = enabled);
                let _ = self.scanner.send(scan::Request::Roots);
                QueryResult::Done
            }
        })
    }

    fn roots_changed(&self, conn: &Connection) -> Result<()> {
        let (roots, tracks) = (db::roots(conn)?, db::track_count(conn)?);
        self.shared.update(|s| {
            s.roots = roots;
            s.tracks = tracks;
            s.revision += 1;
        });
        let _ = self.scanner.send(scan::Request::Roots);
        Ok(())
    }

    /// The engine command that plays what a `Play` or `PlayAlbum` query
    /// selects: the tracks in order as the context, labelled, from the start index.
    pub fn selection(&self, query: &LibraryQuery) -> Result<Command> {
        let conn = self.db();
        let (sql, arg, start, kind) = match query {
            LibraryQuery::Play {
                search,
                sort,
                start,
            } => (
                format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} {}",
                    db::SEARCH,
                    db::order(sort)
                ),
                db::fts_query(search.as_deref()),
                *start,
                ContextKind::List,
            ),
            LibraryQuery::PlayAlbum { album, sort } => (
                format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} = ?1 {}",
                    db::ALBUM_KEY,
                    db::order(sort)
                ),
                Some(album.clone()),
                0,
                ContextKind::Album,
            ),
            _ => return Err(Error::Other("not a play query".into())),
        };
        let mut stmt = conn.prepare(&sql)?;
        let tracks = stmt
            .query_map([arg], library_track)?
            .map(|t| t.map(|t| t.track))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if tracks.is_empty() {
            return Err(Error::Other("nothing to play".into()));
        }
        let label = match query {
            LibraryQuery::Play {
                search: Some(search),
                ..
            } if !search.trim().is_empty() => Some(format!("Search: {}", search.trim())),
            LibraryQuery::Play { .. } => Some("Library".into()),
            _ => tracks[0].album.clone(),
        };
        Ok(Command::SetContext {
            tracks,
            start: Some(start),
            kind,
            label,
        })
    }

    /// A file on this device holding `track`, matched by identity keys (audio
    /// hash, MusicBrainz recording, ISRC) or, failing those, by artist, title
    /// and duration. Used for references queued on other devices.
    pub fn find(&self, track: &TrackRef) -> Option<PathBuf> {
        let conn = self.db();
        let by = |column: &str, value: &Option<String>| -> Option<String> {
            let value = value.as_ref().filter(|v| !v.is_empty())?;
            conn.query_row(
                &format!("SELECT path FROM tracks WHERE {column} = ?1 AND missing = 0 LIMIT 1"),
                [value],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
        };
        let keys = &track.keys;
        let path = by("audio_hash", &keys.audio_hash)
            .or_else(|| by("musicbrainz_recording", &keys.musicbrainz_recording))
            .or_else(|| by("isrc", &keys.isrc))
            .or_else(|| {
                let (title, artist) = (track.title.as_ref()?, track.artist.as_ref()?);
                let duration = track.duration_ms.map_or(-1, |d| d as i64);
                conn.query_row(
                    "SELECT path FROM tracks
                     WHERE missing = 0 AND lower(title) = lower(?1)
                       AND lower(coalesce(artist, album_artist)) = lower(?2)
                       AND (?3 < 0 OR abs(coalesce(duration_ms, ?3) - ?3) <= 2000)
                     ORDER BY lower(coalesce(album, '')) = lower(coalesce(?4, '')) DESC LIMIT 1",
                    params![title.trim(), artist.trim(), duration, track.album],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten()
            })?;
        let path = PathBuf::from(path);
        path.is_file().then_some(path)
    }

    /// The image file for artwork `key`, scaled to fit `size` pixels (made
    /// once and cached) or full size.
    pub fn art(&self, key: &str, size: Option<u32>) -> Result<PathBuf> {
        let source: String = self
            .db()
            .query_row("SELECT path FROM art WHERE key = ?1", [key], |r| r.get(0))
            .optional()?
            .ok_or_else(|| Error::Other("no such artwork".into()))?;
        // Never upscaled: an image no larger than asked for is sent as it is.
        let fits = |(w, h): (u32, u32)| size.is_none_or(|size| w.max(h) <= size);
        match size {
            Some(size) if !image::image_dimensions(&source).is_ok_and(fits) => {
                thumbnail(&self.shared, key, &source, size)
            }
            _ => Ok(source.into()),
        }
    }

    /// Artwork keys for every image of the track at `path`: its own art
    /// first, then the other pictures embedded in it, then the other images
    /// in its folder (back, booklet, disc…). Read when asked, not at scan (#18).
    pub fn images(&self, path: &str) -> Result<Vec<String>> {
        let art: Option<String> = self
            .db()
            .query_row("SELECT art FROM tracks WHERE path = ?1", [path], |r| {
                r.get(0)
            })
            .optional()?
            .ok_or_else(|| Error::Other("not in the library".into()))?;
        let file = Path::new(path);
        let pictures = tags::pictures(file);
        let mut folder: Vec<PathBuf> = file
            .parent()
            .and_then(|dir| fs::read_dir(dir).ok())
            .into_iter()
            .flatten()
            .filter_map(|e| Some(e.ok()?.path()))
            .filter(|p| tags::is_image(p))
            .collect();
        folder.sort();
        let conn = self.db();
        let shown: Option<String> = art.as_ref().and_then(|key| {
            conn.query_row("SELECT path FROM art WHERE key = ?1", [key], |r| r.get(0))
                .ok()
        });
        let mut keys: Vec<String> = art.into_iter().collect();
        let mut add = |key: String| {
            if !keys.contains(&key) {
                keys.push(key);
            }
        };
        for (bytes, ext) in &pictures {
            if let Some(key) = scan::save_embedded_art(&self.shared, &conn, bytes, ext) {
                add(key);
            }
        }
        for image in folder {
            let (Some(image), Ok(meta)) = (image.to_str(), fs::metadata(&image)) else {
                continue;
            };
            if shown.as_deref() == Some(image) {
                continue;
            }
            // Keyed by path, size and modification time rather than content,
            // so large booklet scans aren't read each time the sheet opens.
            let id = format!("{image}\0{}\0{:?}", meta.len(), meta.modified().ok());
            let key = scan::art_key(id.as_bytes());
            conn.execute(
                "INSERT OR IGNORE INTO art (key, path) VALUES (?1, ?2)",
                params![key, image],
            )?;
            add(key);
        }
        Ok(keys)
    }
}

/// The artwork size the player bar asks for. The scanner makes it for all
/// artwork in the background, so the bar's cover shows at once (#59).
pub const BAR_ART_SIZE: u32 = 128;

fn thumb_path(shared: &Shared, key: &str, size: u32) -> PathBuf {
    shared.art_dir.join(format!("{key}-{size}.jpg"))
}

/// Artwork `key` (stored at `source`) scaled to fit `size`, made and cached
/// on first use.
fn thumbnail(shared: &Shared, key: &str, source: &str, size: u32) -> Result<PathBuf> {
    let size = size.clamp(32, 1024);
    let thumb = thumb_path(shared, key, size);
    if !thumb.exists() {
        let image = image::open(source).map_err(|e| Error::Other(e.to_string()))?;
        let mut out = Vec::new();
        image
            .thumbnail(size, size)
            .into_rgb8()
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .map_err(|e| Error::Other(e.to_string()))?;
        // Written aside and renamed, so a request racing the scanner never
        // reads a half-written file.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let part = thumb.with_extension(format!("{}.part", NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::write(&part, out)?;
        fs::rename(&part, &thumb)?;
    }
    Ok(thumb)
}

const TRACK_COLUMNS: &str = "id, path, title, artist, album, album_artist, track_no, disc_no, year, genre, \
     duration_ms, codec, sample_rate, bit_depth, channels, bitrate, musicbrainz_recording, isrc, audio_hash, art, \
     missing, problem";

fn library_track(r: &Row<'_>) -> rusqlite::Result<LibraryTrack> {
    Ok(LibraryTrack {
        id: r.get(0)?,
        track: TrackRef {
            source: "local".into(),
            id: r.get(1)?,
            keys: TrackKeys {
                musicbrainz_recording: r.get(16)?,
                isrc: r.get(17)?,
                audio_hash: r.get::<_, Option<String>>(18)?.filter(|h| !h.is_empty()),
            },
            title: r.get(2)?,
            artist: r.get(3)?,
            album: r.get(4)?,
            duration_ms: r.get::<_, Option<i64>>(10)?.map(|d| d as u64),
        },
        album_artist: r.get(5)?,
        track_no: r.get(6)?,
        disc_no: r.get(7)?,
        year: r.get(8)?,
        genre: r.get(9)?,
        codec: r.get(11)?,
        sample_rate: r.get(12)?,
        bit_depth: r.get(13)?,
        channels: r.get(14)?,
        bitrate: r.get(15)?,
        art: r.get(19)?,
        missing: r.get(20)?,
        problem: r.get(21)?,
    })
}

/// Runs of consecutive tracks from the same album, in list order.
fn groups(conn: &Connection, search: Option<&str>, sort: &[SortRule]) -> Result<Vec<TrackGroup>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, coalesce(album, ''), coalesce(album_artist, artist, ''), coalesce(duration_ms, 0)
         FROM tracks WHERE {} {}",
        db::ALBUM_KEY,
        db::SEARCH,
        db::order(sort)
    ))?;
    let mut rows = stmt.query([db::fts_query(search)])?;
    let mut groups: Vec<TrackGroup> = Vec::new();
    let mut index = 0;
    while let Some(row) = rows.next()? {
        let key: String = row.get(0)?;
        let duration = row.get::<_, i64>(3)? as u64;
        match groups.last_mut() {
            Some(g) if g.key == key => {
                g.count += 1;
                g.duration_ms += duration;
            }
            _ => groups.push(TrackGroup {
                start: index,
                count: 1,
                key,
                title: row.get(1)?,
                artist: row.get(2)?,
                duration_ms: duration,
            }),
        }
        index += 1;
    }
    Ok(groups)
}

/// Where the library and engine keep their data: `MOTIF_DATA_DIR`, else the
/// OS data directory (e.g. `~/.local/share/motif`).
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("MOTIF_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(dirs::data_dir()?.join("motif")))
}

/// Exclusive use of a library directory by one engine. The desktop app and
/// the headless engine share the library on a machine; whichever starts
/// first owns it and the other connects to that engine instead.
pub struct Claim {
    file: fs::File,
}

impl Claim {
    /// Claims `data_dir`, or returns the address the current owner published.
    pub fn take(data_dir: &Path) -> Result<std::result::Result<Self, Option<String>>> {
        fs::create_dir_all(data_dir)?;
        let path = data_dir.join("engine.lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Ok(Self { file })),
            Err(fs::TryLockError::WouldBlock) => {
                let owner = fs::read_to_string(&path)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                Ok(Err(owner))
            }
            Err(fs::TryLockError::Error(err)) => Err(err.into()),
        }
    }

    /// Records where clients reach the owning engine.
    pub fn publish(&mut self, url: &str) -> Result<()> {
        self.file.set_len(0)?;
        std::io::Seek::rewind(&mut self.file)?;
        self.file.write_all(url.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_words_match_word_starts() {
        assert_eq!(
            db::fts_query(Some("beast  nast")).as_deref(),
            Some("\"beast\"* \"nast\"*")
        );
        assert_eq!(db::fts_query(Some("  ")), None);
        assert_eq!(db::fts_query(Some("a\"b")).as_deref(), Some("\"a\"\"b\"*"));
    }
}
