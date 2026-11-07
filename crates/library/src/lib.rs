//! The music library: what files this device has, their tags, artwork and
//! identity keys, in a SQLite database. It knows nothing about playback.
//!
//! A scanner thread keeps the database in line with the library folders
//! (watching them for changes) and fills in audio hashes in the background.
//! Seek bar waveforms are decoded when asked for and cached by audio hash,
//! and so is the loudness of files without ReplayGain tags. Lyrics are read
//! when asked for, LRCLIB's cached by audio hash.

mod db;
mod hash;
mod loudness;
mod lyrics;
mod scan;
mod tags;
mod waveform;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
};

use protocol::{
    Command, ContextKind, ImageKind, LibraryAlbum, LibraryQuery, LibraryStatus, LibraryTrack,
    Lyrics, LyricsLine, LyricsSource, QueryResult, SortRule, TrackGroup, TrackImage, TrackKeys,
    TrackRef,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use tokio::sync::watch;

pub use lyrics::{LRCLIB, tls_roots};

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
    db_path: PathBuf,
    /// Images listed by [`Library::images`] (key → file), kept in memory so
    /// listing them never waits on the scanner's writes.
    extras: Mutex<HashMap<String, PathBuf>>,
    /// Files waiting for their loudness to be measured, and whether a
    /// thread is measuring them (see [`loudness::request`]).
    measuring: Mutex<(VecDeque<PathBuf>, bool)>,
    /// Files the library's decoder can't measure (those only libopus or
    /// FFmpeg play), so they aren't tried again on every engine change.
    unmeasurable: Mutex<HashSet<PathBuf>>,
    /// Set when a measurement is cached, until [`Library::take_measured`].
    measured: AtomicBool,
    /// When LRCLIB last failed (see [`lyrics::BACKOFF`]).
    lrclib_failed: Mutex<Option<std::time::Instant>>,
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
            db_path: db_path.clone(),
            extras: Mutex::default(),
            measuring: Mutex::default(),
            unmeasurable: Mutex::default(),
            measured: AtomicBool::new(false),
            lrclib_failed: Mutex::default(),
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

    /// Holds (or lets go of) background work: audio hashing and thumbnails.
    /// Scans still run. Safe to repeat.
    pub fn hold_background(&self, held: bool) {
        let _ = self.scanner.send(scan::Request::Hold(held));
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
        if let LibraryQuery::Waveform { path } = &query {
            return Ok(QueryResult::Waveform {
                levels: self.waveform(path)?,
            });
        }
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
                    "SELECT {key} AS k, coalesce(album, ''), coalesce(album_artist, artist, ''), min(coalesce(original_year, year)),
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
            LibraryQuery::Waveform { .. } => unreachable!("answered above"),
            LibraryQuery::Play { .. } | LibraryQuery::PlayAlbum { .. } => {
                return Err(Error::Other("play queries go through selection()".into()));
            }
            LibraryQuery::Lyrics { .. } | LibraryQuery::SaveLyrics { .. } => {
                return Err(Error::Other(
                    "lyrics queries go through lyrics() and save_lyrics()".into(),
                ));
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

    /// The ReplayGain values for the file at `path`: its tags, else its
    /// measured loudness (album gain once every track of its album is
    /// measured or tagged). All `None` if it isn't in the library or has
    /// neither.
    pub fn replay_gain(&self, path: &Path) -> protocol::ReplayGain {
        let Some(path) = path.to_str() else {
            return Default::default();
        };
        let conn = self.db();
        // Each track's tags, measurement and duration.
        let row = |r: &Row| -> rusqlite::Result<_> {
            let tags: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) =
                (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
            let measured: Option<(Option<f64>, f64)> = r
                .get::<_, Option<f64>>(5)?
                .map(|peak| (r.get(4).ok().flatten(), peak));
            Ok((tags, measured, r.get::<_, Option<f64>>(6)?.unwrap_or(0.0)))
        };
        let select = format!(
            "SELECT rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak,
                    l.lufs, l.peak, duration_ms, {} FROM tracks
             LEFT JOIN loudness l ON l.hash = audio_hash",
            db::ALBUM_KEY
        );
        let Ok((tags, measured, _, album)) =
            conn.query_row(&format!("{select} WHERE path = ?1"), [path], |r| {
                let (tags, measured, ms) = row(r)?;
                Ok((tags, measured, ms, r.get::<_, String>(7)?))
            })
        else {
            return Default::default();
        };
        let f32s = |v: Option<f64>| v.map(|v| v as f32);
        if tags.0.is_some() || measured.is_none() {
            return protocol::ReplayGain {
                track_gain: f32s(tags.0),
                track_peak: f32s(tags.1),
                album_gain: f32s(tags.2),
                album_peak: f32s(tags.3),
            };
        }
        let (lufs, peak) = measured.unwrap_or_default();
        // Every track's loudness, from its tag or measurement, or None if one is unknown.
        let tracks: Option<Vec<_>> = conn
            .prepare(&format!(
                "{select} WHERE missing = 0 AND {} = ?1",
                db::ALBUM_KEY
            ))
            .and_then(|mut stmt| {
                stmt.query_map([&album], |r| {
                    let ((gain, tag_peak, ..), measured, ms) = row(r)?;
                    Ok(match (gain, measured) {
                        (Some(gain), _) => Some((
                            Some(loudness::REFERENCE - gain),
                            tag_peak.unwrap_or(0.0),
                            ms,
                        )),
                        (None, Some((lufs, peak))) => Some((lufs, peak, ms)),
                        (None, None) => None,
                    })
                })?
                .collect::<rusqlite::Result<_>>()
            })
            .unwrap_or_default();
        let album = tracks.as_deref().and_then(loudness::album);
        protocol::ReplayGain {
            track_gain: f32s(lufs.map(|l| loudness::REFERENCE - l)),
            track_peak: Some(peak as f32),
            album_gain: f32s(album.map(|a| a.0)),
            album_peak: f32s(album.map(|a| a.1)),
        }
    }

    /// Measures the loudness of the library files at `paths` that have no
    /// ReplayGain tags or cached measurement, in order on a background
    /// thread. With `wait`, the first is measured before this returns (up to
    /// a few seconds), for a track about to play.
    pub fn measure(&self, paths: Vec<PathBuf>, wait: bool) {
        loudness::request(&self.shared, paths, wait);
    }

    /// Whether a measurement was cached since the last call, which can
    /// change what [`Self::replay_gain`] returns.
    pub fn take_measured(&self) -> bool {
        self.shared.measured.swap(false, Ordering::Relaxed)
    }

    /// The MusicBrainz release (album) ID tagged on the track at `path`.
    pub fn musicbrainz_album(&self, path: &str) -> Option<String> {
        self.db()
            .query_row(
                "SELECT musicbrainz_album FROM tracks WHERE path = ?1",
                [path],
                |r| r.get(0),
            )
            .ok()
            .flatten()
    }

    /// The seek bar waveform of the track at `path` (see [`waveform::compute`]),
    /// cached by audio hash or decoded now, which takes a moment for a long
    /// file. The database isn't locked meanwhile.
    pub fn waveform(&self, path: &str) -> Result<Vec<u8>> {
        let known: Option<String> = self
            .db()
            .query_row(
                "SELECT audio_hash FROM tracks WHERE path = ?1",
                [path],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::Other("not in the library".into()))?;
        if known.as_deref() == Some("") {
            return Err(Error::Other("the file can't be decoded".into()));
        }
        // Not hashed yet: a FLAC's hash is free, others are hashed while decoding.
        let hash = known.or_else(|| hash::flac_hash(Path::new(path)));
        if let Some(hash) = &hash {
            let cached = self
                .db()
                .query_row(
                    "SELECT levels FROM waveforms WHERE hash = ?1",
                    [hash],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(levels) = cached {
                return Ok(levels);
            }
        }
        let (levels, pcm_hash) =
            waveform::compute(Path::new(path), hash.is_none()).map_err(Error::Other)?;
        let Some(hash) = hash.or(pcm_hash) else {
            return Ok(levels);
        };
        let stored = levels.clone();
        self.write_aside("waveform", move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO waveforms (hash, levels) VALUES (?1, ?2)",
                params![hash, stored],
            )
        });
        Ok(levels)
    }

    /// Runs `write` on a connection and thread of its own: the scanner may
    /// hold the write lock for seconds, and neither queries nor the answer
    /// that made `what` wait.
    fn write_aside(
        &self,
        what: &'static str,
        write: impl FnOnce(&Connection) -> rusqlite::Result<usize> + Send + 'static,
    ) {
        let db_path = self.shared.db_path.clone();
        std::thread::spawn(move || {
            let store = || -> rusqlite::Result<()> {
                let conn = Connection::open(db_path)?;
                conn.busy_timeout(std::time::Duration::from_secs(30))?;
                write(&conn)?;
                Ok(())
            };
            if let Err(err) = store() {
                tracing::warn!(%err, "can't cache {what}");
            }
        });
    }

    /// The lyrics of the track at `path` (see [`LibraryQuery::Lyrics`]), from
    /// the LRCLIB server at `lrclib` if it is on and nothing local has them.
    /// The database isn't locked while LRCLIB answers.
    pub fn lyrics(&self, path: &str, lrclib: Option<&str>) -> Result<Option<Lyrics>> {
        type Row = (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
        );
        let (title, artist, album, duration_ms, hash): Row = self
            .db()
            .query_row(
                "SELECT title, coalesce(artist, album_artist), album, duration_ms, audio_hash
                 FROM tracks WHERE path = ?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(|| Error::Other("not in the library".into()))?;
        let file = Path::new(path);
        // Sidecars beat embedded lyrics, but synced beats plain first (#90).
        let sidecars = lyrics::sidecars(file).into_iter();
        let embedded = lyrics::embedded(file).into_iter();
        let local = lyrics::best(
            (sidecars.map(|l| (LyricsSource::Sidecar, l)))
                .chain(embedded.map(|l| (LyricsSource::Embedded, l))),
        );
        let found = |best: Option<(LyricsSource, Vec<LyricsLine>)>| {
            Ok(best.map(|(source, lines)| Lyrics { source, lines }))
        };
        if local
            .as_ref()
            .is_some_and(|(_, lines)| lyrics::synced(lines))
        {
            return found(local);
        }
        let (Some(base), Some(title), Some(artist)) = (lrclib, title, artist) else {
            return found(local);
        };
        // Not hashed yet: a FLAC's hash is free; others are asked for uncached
        // until the scanner hashes them.
        let hash = hash
            .filter(|h| !h.is_empty())
            .or_else(|| hash::flac_hash(file));
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let cached: Option<(Option<String>, Option<String>, i64)> = match &hash {
            Some(hash) => self
                .db()
                .query_row(
                    "SELECT synced, plain, fetched FROM lyrics WHERE hash = ?1",
                    [hash],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?,
            None => None,
        };
        let (synced, plain) = match cached {
            Some((synced, plain, fetched))
                if synced.is_some() || plain.is_some() || now - fetched < lyrics::RETRY_SECS =>
            {
                (synced, plain)
            }
            _ => {
                let failed = *self.shared.lrclib_failed.lock().unwrap();
                if failed.is_some_and(|at| at.elapsed() < lyrics::BACKOFF) {
                    return found(local);
                }
                let duration = duration_ms.map(|d| d.max(0) as u64);
                let fetched = match lyrics::fetch(base, &artist, &title, album.as_deref(), duration)
                {
                    Ok(fetched) => fetched.unwrap_or_default(),
                    Err(err) => {
                        tracing::warn!(path, %err, "can't ask LRCLIB for lyrics");
                        *self.shared.lrclib_failed.lock().unwrap() =
                            Some(std::time::Instant::now());
                        return found(local);
                    }
                };
                if let Some(hash) = hash {
                    let stored = fetched.clone();
                    self.write_aside("lyrics", move |conn| {
                        conn.execute(
                            "INSERT OR REPLACE INTO lyrics (hash, synced, plain, fetched)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![hash, stored.0, stored.1, now],
                        )
                    });
                }
                fetched
            }
        };
        // Local plain lyrics beat LRCLIB's plain ones, not its synced ones.
        let fetched = [synced, plain]
            .into_iter()
            .flatten()
            .map(|text| (LyricsSource::Lrclib, lyrics::parse(&text)));
        found(lyrics::best(local.into_iter().chain(fetched)))
    }

    /// Writes the lyrics [`Self::lyrics`] finds as `<track>.lrc` next to the
    /// file, every time moved by `offset_ms` (positive is later).
    pub fn save_lyrics(&self, path: &str, offset_ms: i32, lrclib: Option<&str>) -> Result<()> {
        let lyrics = self
            .lyrics(path, lrclib)?
            .ok_or_else(|| Error::Other("no lyrics to save".into()))?;
        let file = Path::new(path).with_extension("lrc");
        let text = match fs::read_to_string(&file) {
            // The lyrics came from this file: move its offset, keeping word stamps and tags.
            Ok(existing)
                if lyrics.source == LyricsSource::Sidecar
                    && lyrics::synced(&lyrics::parse(&existing)) =>
            {
                lyrics::shift(&existing, offset_ms)
            }
            Ok(_) => {
                return Err(Error::Other(format!(
                    "{} already exists; not replacing it",
                    file.display()
                )));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                lyrics::to_lrc(&lyrics.lines, offset_ms)
            }
            // Unreadable (not UTF-8, say): leave it alone too.
            Err(err) => return Err(Error::Other(format!("{}: {err}", file.display()))),
        };
        fs::write(&file, text)
            .map_err(|e| Error::Other(format!("can't save {}: {e}", file.display())))
    }

    /// The image file for artwork `key`, scaled to fit `size` pixels (made
    /// once and cached) or full size. A picture embedded in a track has no
    /// file of its own at full size: [`Self::art_image`] reads it.
    pub fn art(&self, key: &str, size: Option<u32>) -> Result<PathBuf> {
        let source = self.art_source(key)?;
        let embedded = tags::is_audio(&source);
        // Never upscaled: an image no larger than asked for is sent as it is.
        let fits = |(w, h): (u32, u32)| size.is_none_or(|size| w.max(h) <= size);
        match size {
            Some(size) if embedded || !image::image_dimensions(&source).is_ok_and(fits) => {
                thumbnail(&self.shared, key, &source, size)
            }
            None if embedded => Err(Error::Other("embedded artwork has no file".into())),
            _ => Ok(source),
        }
    }

    /// Artwork `key` as [`Self::art`] gives it, read, with its media type.
    pub fn art_image(&self, key: &str, size: Option<u32>) -> Result<(Vec<u8>, &'static str)> {
        let bytes = match size {
            Some(_) => fs::read(self.art(key, size)?)?,
            None => art_bytes(key, &self.art_source(key)?)?,
        };
        let png = bytes.starts_with(b"\x89PNG");
        Ok((bytes, if png { "image/png" } else { "image/jpeg" }))
    }

    /// Where artwork `key` comes from: an image file, or a track it is embedded in.
    fn art_source(&self, key: &str) -> Result<PathBuf> {
        let stored: Option<String> = self
            .db()
            .query_row("SELECT path FROM art WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        stored
            .map(PathBuf::from)
            .or_else(|| self.shared.extras.lock().unwrap().get(key).cloned())
            .ok_or_else(|| Error::Other("no such artwork".into()))
    }

    /// Every image of the track at `path`, named when it says what it shows:
    /// its own art first, then the other pictures embedded in it, then the
    /// other images in its folder (back, booklet, disc…). Read when asked,
    /// not at scan (#18).
    pub fn images(&self, path: &str) -> Result<Vec<TrackImage>> {
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
        let shown: Option<String> = art.as_ref().and_then(|key| {
            self.db()
                .query_row("SELECT path FROM art WHERE key = ?1", [key], |r| r.get(0))
                .ok()
        });
        // Only read the database: the scanner may hold its write lock for
        // seconds, so new keys go in memory for `art` to find.
        let mut found: Vec<(String, PathBuf, Option<ImageKind>)> = pictures
            .iter()
            .map(|(bytes, _, kind)| (scan::art_key(bytes), file.to_path_buf(), *kind))
            .collect();
        for image in folder {
            let (Some(name), Ok(meta)) = (image.to_str(), fs::metadata(&image)) else {
                continue;
            };
            if shown.as_deref() == Some(name) {
                continue;
            }
            // Keyed by content like embedded art, so a copy of a track's
            // cover shows once; large booklet scans by path, size and
            // modification time instead, so they aren't read each time.
            let small = meta.len() <= CONTENT_KEY_LIMIT;
            let key = match small.then(|| fs::read(&image).ok()).flatten() {
                Some(bytes) => scan::art_key(&bytes),
                None => {
                    let id = format!("{name}\0{}\0{:?}", meta.len(), meta.modified().ok());
                    scan::art_key(id.as_bytes())
                }
            };
            let kind = tags::image_kind(&image);
            found.push((key, image, kind));
        }
        let mut images: Vec<TrackImage> = art
            .map(|key| {
                // Named by its picture type if embedded, else by its file name.
                let kind = match found.iter().find(|(k, ..)| *k == key) {
                    Some((.., kind)) => *kind,
                    None => shown
                        .as_deref()
                        .and_then(|p| tags::image_kind(Path::new(p))),
                };
                TrackImage { key, kind }
            })
            .into_iter()
            .collect();
        let mut extras = self.shared.extras.lock().unwrap();
        for (key, file, kind) in found {
            if !images.iter().any(|i| i.key == key) {
                extras.insert(key.clone(), file);
                images.push(TrackImage { key, kind });
            }
        }
        Ok(images)
    }
}

/// Folder images up to this size are keyed by their content (see
/// [`Library::images`]).
const CONTENT_KEY_LIMIT: u64 = 2 << 20;

/// The artwork size the player bar asks for. The scanner makes it for all
/// artwork in the background, so the bar's cover shows at once (#59).
pub const BAR_ART_SIZE: u32 = 128;

fn thumb_path(shared: &Shared, key: &str, size: u32) -> PathBuf {
    shared.art_dir.join(format!("{key}-{size}.jpg"))
}

/// Artwork `key` (stored at `source`) scaled to fit `size`, made and cached
/// on first use.
fn thumbnail(shared: &Shared, key: &str, source: &Path, size: u32) -> Result<PathBuf> {
    let size = size.clamp(32, 1024);
    let thumb = thumb_path(shared, key, size);
    if !thumb.exists() {
        let image = image::load_from_memory(&art_bytes(key, source)?)
            .map_err(|e| Error::Other(e.to_string()))?;
        // Embedded pictures get here at any size: never upscaled either.
        let image = if image.width().max(image.height()) > size {
            image.thumbnail(size, size)
        } else {
            image
        };
        let mut out = Vec::new();
        image
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

/// The bytes of artwork `key` stored at `source`: an image file, or a
/// picture embedded in an audio file.
fn art_bytes(key: &str, source: &Path) -> Result<Vec<u8>> {
    if !tags::is_audio(source) {
        return Ok(fs::read(source)?);
    }
    tags::embedded(source, key).ok_or_else(|| Error::Other("artwork not in its file".into()))
}

const TRACK_COLUMNS: &str = "id, path, title, artist, album, album_artist, track_no, disc_no, year, genre, \
     duration_ms, codec, sample_rate, bit_depth, channels, bitrate, musicbrainz_recording, isrc, audio_hash, art, \
     missing, problem, original_year";

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
        original_year: r.get(22)?,
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
