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
    sync::{Arc, Mutex, mpsc},
};

use protocol::{
    LibraryAlbum, LibraryQuery, LibraryStatus, LibraryTrack, QueryResult, TrackGroup, TrackKeys,
    TrackRef,
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
                    db::ORDER
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
            LibraryQuery::Groups { search } => QueryResult::Groups {
                groups: groups(&conn, search.as_deref())?,
            },
            LibraryQuery::Albums {
                search,
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
                     FROM tracks WHERE {search} GROUP BY k
                     ORDER BY lower(coalesce(album_artist, artist, '')), lower(coalesce(album, ''))
                     LIMIT ?2 OFFSET ?3",
                    key = db::ALBUM_KEY,
                    search = db::SEARCH
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
            LibraryQuery::AlbumPosition { search, album } => QueryResult::Position {
                index: groups(&conn, search.as_deref())?
                    .into_iter()
                    .find(|g| g.key == album)
                    .map(|g| g.start),
            },
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
                conn.execute("DELETE FROM roots WHERE path = ?1", [&path])?;
                conn.execute(
                    "DELETE FROM tracks WHERE substr(path, 1, length(?1)) = ?1",
                    [&path],
                )?;
                self.roots_changed(&conn)?;
                QueryResult::Done
            }
            LibraryQuery::Rescan => {
                let _ = self.scanner.send(scan::Request::Rescan);
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

    /// The tracks a `Play` or `PlayAlbum` query selects, in order, and the
    /// index to start at.
    pub fn selection(&self, query: &LibraryQuery) -> Result<(Vec<TrackRef>, u32)> {
        let conn = self.db();
        let (sql, arg, start) = match query {
            LibraryQuery::Play { search, start } => (
                format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} {}",
                    db::SEARCH,
                    db::ORDER
                ),
                db::fts_query(search.as_deref()),
                *start,
            ),
            LibraryQuery::PlayAlbum { album } => (
                format!(
                    "SELECT {TRACK_COLUMNS} FROM tracks WHERE {} = ?1 {}",
                    db::ALBUM_KEY,
                    db::ORDER
                ),
                Some(album.clone()),
                0,
            ),
            _ => return Err(Error::Other("not a play query".into())),
        };
        let mut stmt = conn.prepare(&sql)?;
        let tracks = stmt
            .query_map([arg], library_track)?
            .map(|t| t.map(|t| t.track))
            .collect::<rusqlite::Result<_>>()?;
        Ok((tracks, start))
    }

    /// A file on this device holding `track`, matched by identity keys (audio
    /// hash, MusicBrainz recording, ISRC) or, failing those, by artist, title
    /// and duration. Used for references queued on other devices.
    pub fn find(&self, track: &TrackRef) -> Option<PathBuf> {
        let conn = self.db();
        let by = |column: &str, value: &Option<String>| -> Option<String> {
            let value = value.as_ref().filter(|v| !v.is_empty())?;
            conn.query_row(
                &format!("SELECT path FROM tracks WHERE {column} = ?1 LIMIT 1"),
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
                     WHERE lower(title) = lower(?1) AND lower(coalesce(artist, album_artist)) = lower(?2)
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
        let Some(size) = size else {
            return Ok(source.into());
        };
        let size = size.clamp(32, 1024);
        let thumb = self.shared.art_dir.join(format!("{key}-{size}.jpg"));
        if !thumb.exists() {
            let image = image::open(&source).map_err(|e| Error::Other(e.to_string()))?;
            let mut out = Vec::new();
            image
                .thumbnail(size, size)
                .into_rgb8()
                .write_to(
                    &mut std::io::Cursor::new(&mut out),
                    image::ImageFormat::Jpeg,
                )
                .map_err(|e| Error::Other(e.to_string()))?;
            fs::write(&thumb, out)?;
        }
        Ok(thumb)
    }
}

const TRACK_COLUMNS: &str = "id, path, title, artist, album, album_artist, track_no, disc_no, year, genre, \
     duration_ms, codec, sample_rate, bit_depth, channels, bitrate, musicbrainz_recording, isrc, audio_hash, art";

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
    })
}

/// Runs of consecutive tracks from the same album, in list order.
fn groups(conn: &Connection, search: Option<&str>) -> Result<Vec<TrackGroup>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, coalesce(album, ''), coalesce(album_artist, artist, ''), coalesce(duration_ms, 0)
         FROM tracks WHERE {} {}",
        db::ALBUM_KEY,
        db::SEARCH,
        db::ORDER
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
