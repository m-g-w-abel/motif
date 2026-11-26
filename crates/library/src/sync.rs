//! Metadata sync between linked engines (#239). Every change to playlists,
//! plays and marks is an op in the `ops` table, numbered per engine
//! (`device`, its key) and stamped with a Lamport time (`at`, ms: later than
//! every op it has seen). Engines swap the ops the other lacks ([`Versions`])
//! and apply them in `at` order, which keeps causes before their effects, so
//! changes spread through engines in between. Plays are a grow-only set;
//! names, positions and marks are last-writer-wins by (`at`, device);
//! deletes are tombstones. Paths never leave the engine: tracks travel as
//! [`Track`].

use std::collections::BTreeMap;

use protocol::TrackRef;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Library, Result};

pub(crate) const SCHEMA: &str = "
    -- The op log: change `seq` (from 1) of the engine `device` ('' until the
    -- library knows its key, see set_device), `op` as JSON.
    CREATE TABLE IF NOT EXISTS ops (
        device TEXT NOT NULL, seq INTEGER NOT NULL, at INTEGER NOT NULL, op TEXT NOT NULL,
        PRIMARY KEY (device, seq)
    );
    CREATE INDEX IF NOT EXISTS ops_at ON ops (at);
    -- The stamp of the op that last set each last-writer-wins field
    -- (`name/<playlist>`, `pos/<playlist>/<item>`, `favourite/<ratings key>`,
    -- `rating/<ratings key>`), and tombstones (`deleted/<playlist>`,
    -- `removed/<playlist>/<item>`).
    CREATE TABLE IF NOT EXISTS stamps (key TEXT PRIMARY KEY, at INTEGER NOT NULL, device TEXT NOT NULL);
    -- Plays (the op `device`/`seq` that logged them) a scrobbling service has
    -- been given or can't take (#240).
    CREATE TABLE IF NOT EXISTS scrobbled (
        service TEXT NOT NULL, device TEXT NOT NULL, seq INTEGER NOT NULL,
        PRIMARY KEY (service, device, seq)
    );";

/// A track as other engines can find it ([`locate`]): never its path.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Track {
    pub hash: Option<String>,
    pub musicbrainz: Option<String>,
    pub isrc: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
}

impl From<&TrackRef> for Track {
    fn from(track: &TrackRef) -> Self {
        Self {
            hash: track.keys.audio_hash.clone(),
            musicbrainz: track.keys.musicbrainz_recording.clone(),
            isrc: track.keys.isrc.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms,
        }
    }
}

/// One change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Op {
    /// A listen that started at `started` (Unix seconds).
    Play {
        track: Track,
        started: u64,
    },
    /// Creates or renames a playlist.
    Name {
        playlist: String,
        name: String,
    },
    Delete {
        playlist: String,
    },
    Add {
        playlist: String,
        item: String,
        track: Track,
        pos: f64,
    },
    Remove {
        playlist: String,
        item: String,
    },
    Move {
        playlist: String,
        item: String,
        pos: f64,
    },
    Favourite {
        track: Track,
        on: bool,
    },
    /// Half stars, 1 to 10; `None` clears it.
    Rating {
        track: Track,
        rating: Option<u8>,
    },
    /// Play `seq` of `device` is done with for `service`: sent, or not sendable.
    Scrobbled {
        service: String,
        device: String,
        seq: u64,
    },
}

/// A play a service may still lack ([`Library::unscrobbled`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Unscrobbled {
    pub device: String,
    pub seq: u64,
    pub started: u64,
    pub track: Track,
}

/// An op as the log holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Logged {
    pub device: String,
    pub seq: u64,
    pub at: u64,
    pub op: Op,
}

/// The last op seen from each engine, by key.
pub type Versions = BTreeMap<String, u64>;

impl Library {
    /// Logs this engine's changes under `key` from now on. Changes made
    /// before it was known become `key`'s; the first time, what the library
    /// already holds becomes ops too, so it syncs. Safe to repeat.
    pub fn set_device(&self, key: &str) -> Result<()> {
        if key.is_empty() {
            return Err(Error::Other("an engine needs a key".into()));
        }
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let seeded = tx
            .query_row(
                "SELECT 1 FROM settings WHERE key = 'synced'",
                [],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if seeded {
            tx.execute(
                "UPDATE ops SET device = ?1, seq = seq + ?2 WHERE device = ''",
                params![key, last_seq(&tx, key)? as i64],
            )?;
        } else {
            // What those changes did is in the tables, which seed() logs.
            tx.execute("DELETE FROM ops WHERE device = ''", [])?;
            seed(&tx, key)?;
            tx.execute(
                "INSERT INTO settings (key, value) VALUES ('synced', 'on')",
                [],
            )?;
        }
        // For the engines a track's audio is on (`LibraryTrack::devices`).
        tx.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('device', ?1)",
            [key],
        )?;
        tx.commit()?;
        // Still holding the database, so no change is logged in between.
        *self.shared.device.lock().unwrap() = key.into();
        Ok(())
    }

    pub fn versions(&self) -> Result<Versions> {
        let conn = self.db();
        let mut stmt =
            conn.prepare("SELECT device, max(seq) FROM ops WHERE device != '' GROUP BY device")?;
        let versions = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(versions)
    }

    /// Up to `limit` ops an engine that has seen `theirs` lacks, oldest first.
    pub fn ops_since(&self, theirs: &Versions, limit: u32) -> Result<Vec<Logged>> {
        let theirs = serde_json::to_string(theirs).map_err(|e| Error::Other(e.to_string()))?;
        let conn = self.db();
        let mut stmt = conn.prepare(
            // Ranges of the primary key, so an engine that is up to date costs
            // one pass over the engines, not over the log.
            "WITH seen (device, seq) AS (
                 SELECT device, coalesce((SELECT value FROM json_each(?1) WHERE key = device), 0)
                 FROM ops WHERE device != '' GROUP BY device
             )
             SELECT ops.device, ops.seq, ops.at, ops.op FROM seen
             JOIN ops ON ops.device = seen.device AND ops.seq > seen.seq
             ORDER BY ops.at, ops.device, ops.seq LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![theirs, limit], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|(device, seq, at, op)| {
                Ok(Logged {
                    device,
                    seq: seq as u64,
                    at: at as u64,
                    op: serde_json::from_str(&op).map_err(|e| Error::Other(e.to_string()))?,
                })
            })
            .collect()
    }

    /// Applies ops from another engine. Ones already here are skipped, so a
    /// sync interrupted halfway resumes; each engine's must come in order.
    pub fn merge(&self, ops: Vec<Logged>) -> Result<()> {
        let (mut plays, mut playlists, mut ratings) = (false, false, false);
        {
            let conn = self.db();
            let tx = conn.unchecked_transaction()?;
            for logged in ops {
                let Logged {
                    device,
                    seq,
                    at,
                    op,
                } = logged;
                let last = last_seq(&tx, &device)?;
                if device.is_empty() || seq > last + 1 {
                    return Err(Error::Other(format!(
                        "op {seq} of {device:?} comes before {}",
                        last + 1
                    )));
                }
                if seq <= last {
                    continue;
                }
                insert(&tx, &device, seq, at, &op)?;
                if apply(
                    &tx,
                    &op,
                    &Stamp {
                        at,
                        device: &device,
                    },
                    None,
                )? {
                    match op {
                        Op::Play { .. } => plays = true,
                        Op::Favourite { .. } | Op::Rating { .. } => ratings = true,
                        // Nothing a client shows.
                        Op::Scrobbled { .. } => {}
                        _ => playlists = true,
                    }
                }
            }
            tx.commit()?;
        }
        if plays || playlists || ratings {
            self.shared.update(|s| {
                s.plays += plays as u64;
                s.playlists += playlists as u64;
                s.ratings += ratings as u64;
            });
        }
        Ok(())
    }

    /// Plays from every engine that started at or after `since` (Unix
    /// seconds) and that `service` hasn't been given, oldest first. Empty
    /// until [`Self::set_device`], as marks need the plays' ids.
    pub fn unscrobbled(&self, service: &str, since: u64) -> Result<Vec<Unscrobbled>> {
        if self.device().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db();
        // A play is logged after it started, so `at` bounds it and uses the index.
        let mut stmt = conn.prepare(
            "SELECT device, seq, op FROM ops
             WHERE at >= ?2 AND json_extract(op, '$.type') = 'play'
               AND json_extract(op, '$.started') >= ?3
               AND NOT EXISTS (SELECT 1 FROM scrobbled s
                               WHERE s.service = ?1 AND s.device = ops.device AND s.seq = ops.seq)
             ORDER BY json_extract(op, '$.started'), device, seq",
        )?;
        let rows = stmt
            .query_map(params![service, since as i64 * 1000, since as i64], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))
            })?
            .collect::<rusqlite::Result<Vec<(String, i64, String)>>>()?;
        rows.into_iter()
            .filter_map(|(device, seq, op)| match serde_json::from_str(&op) {
                Ok(Op::Play { track, started }) => Some(Ok(Unscrobbled {
                    device,
                    seq: seq as u64,
                    started,
                    track,
                })),
                Ok(_) => None,
                Err(e) => Some(Err(Error::Other(e.to_string()))),
            })
            .collect()
    }

    /// Notes that `service` is done with a play from [`Self::unscrobbled`],
    /// for every engine; whether that was news. Safe to repeat.
    pub fn mark_scrobbled(&self, service: &str, device: &str, seq: u64) -> Result<bool> {
        let op = Op::Scrobbled {
            service: service.into(),
            device: device.into(),
            seq,
        };
        let conn = self.db();
        let tx = conn.unchecked_transaction()?;
        let done = tx
            .query_row(
                "SELECT 1 FROM scrobbled WHERE service = ?1 AND device = ?2 AND seq = ?3",
                params![service, device, seq as i64],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        let added = !done && record(&tx, &self.device(), op, None)?;
        tx.commit()?;
        Ok(added)
    }

    /// The key changes here are logged under ('' until [`Self::set_device`]).
    pub fn device(&self) -> String {
        self.shared.device.lock().unwrap().clone()
    }
}

/// When an op was made, which decides between last writers.
struct Stamp<'a> {
    at: u64,
    device: &'a str,
}

fn last_seq(conn: &Connection, device: &str) -> Result<u64> {
    let last: i64 = conn.query_row(
        "SELECT coalesce(max(seq), 0) FROM ops WHERE device = ?1",
        [device],
        |r| r.get(0),
    )?;
    Ok(last as u64)
}

fn insert(conn: &Connection, device: &str, seq: u64, at: u64, op: &Op) -> Result<()> {
    let op = serde_json::to_string(op).map_err(|e| Error::Other(e.to_string()))?;
    conn.execute(
        "INSERT INTO ops (device, seq, at, op) VALUES (?1, ?2, ?3, ?4)",
        params![device, seq as i64, at as i64, op],
    )?;
    Ok(())
}

/// Logs a change made here as `device`'s and applies it; whether it changed
/// anything. `here` is the file it is about, if any. Callers leave out
/// changes that change nothing, so repeated commands log nothing.
pub(crate) fn record(conn: &Connection, device: &str, op: Op, here: Option<&str>) -> Result<bool> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let latest: Option<i64> = conn.query_row("SELECT max(at) FROM ops", [], |r| r.get(0))?;
    let at = now.max(latest.map_or(0, |l| l + 1)) as u64;
    let seq = last_seq(conn, device)? + 1;
    insert(conn, device, seq, at, &op)?;
    apply(conn, &op, &Stamp { at, device }, here)
}

/// Whether the op stamped `stamp` is newer than what last set `key`.
fn wins(conn: &Connection, key: &str, stamp: &Stamp) -> Result<bool> {
    let old: Option<(i64, String)> = conn
        .query_row("SELECT at, device FROM stamps WHERE key = ?1", [key], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    Ok(old.is_none_or(|(at, device)| (stamp.at as i64, stamp.device) > (at, device.as_str())))
}

fn set_stamp(conn: &Connection, key: &str, stamp: &Stamp) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO stamps (key, at, device) VALUES (?1, ?2, ?3)",
        params![key, stamp.at as i64, stamp.device],
    )?;
    Ok(())
}

pub(crate) fn tombstoned(conn: &Connection, key: &str) -> Result<bool> {
    let found = conn
        .query_row("SELECT 1 FROM stamps WHERE key = ?1", [key], |_| Ok(()))
        .optional()?;
    Ok(found.is_some())
}

/// The file's audio hash, if the scanner has made it.
fn hash_of(conn: &Connection, path: &str) -> Result<Option<String>> {
    let hash = conn
        .query_row(
            "SELECT nullif(audio_hash, '') FROM tracks WHERE path = ?1",
            [path],
            |r| r.get(0),
        )
        .optional()?;
    Ok(hash.flatten())
}

/// The track at `path` as other engines can find it, if it is in the library.
pub(crate) fn track_at(conn: &Connection, path: &str) -> Result<Option<Track>> {
    let track = conn
        .query_row(
            "SELECT nullif(audio_hash, ''), musicbrainz_recording, isrc, title, artist, album, duration_ms
             FROM tracks WHERE path = ?1",
            [path],
            |r| {
                Ok(Track {
                    hash: r.get(0)?,
                    musicbrainz: r.get(1)?,
                    isrc: r.get(2)?,
                    title: r.get(3)?,
                    artist: r.get(4)?,
                    album: r.get(5)?,
                    duration_ms: r.get::<_, Option<i64>>(6)?.map(|d| d as u64),
                })
            },
        )
        .optional()?;
    Ok(track)
}

/// A present track holding `track`, matched by audio hash, MusicBrainz
/// recording or ISRC, else by artist, title and duration; a file here
/// before another engine's copy (catalog.rs).
pub(crate) fn locate(conn: &Connection, track: &Track) -> Option<String> {
    let by = |column: &str, value: &Option<String>| -> Option<String> {
        let value = value.as_ref().filter(|v| !v.is_empty())?;
        conn.query_row(
            &format!("SELECT path FROM tracks WHERE {column} = ?1 AND missing = 0 ORDER BY device LIMIT 1"),
            [value],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    };
    by("audio_hash", &track.hash)
        .or_else(|| by("musicbrainz_recording", &track.musicbrainz))
        .or_else(|| by("isrc", &track.isrc))
        .or_else(|| {
            let (title, artist) = (track.title.as_ref()?, track.artist.as_ref()?);
            let duration = track.duration_ms.map_or(-1, |d| d as i64);
            conn.query_row(
                "SELECT path FROM tracks
                 WHERE missing = 0 AND lower(title) = lower(?1)
                   AND lower(coalesce(artist, album_artist)) = lower(?2)
                   AND (?3 < 0 OR abs(coalesce(duration_ms, ?3) - ?3) <= 2000)
                 ORDER BY device, lower(coalesce(album, '')) = lower(coalesce(?4, '')) DESC LIMIT 1",
                params![title.trim(), artist.trim(), duration, track.album],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
        })
}

/// Applies `op` stamped `stamp`; whether it changed anything. `here` is the
/// file it is about when it was made here; other engines' ops find theirs.
fn apply(conn: &Connection, op: &Op, stamp: &Stamp, here: Option<&str>) -> Result<bool> {
    // The file this engine has of `track` and its audio hash, else just the hash.
    let file = |track: &Track| -> Result<(Option<String>, Option<String>)> {
        match here.map(str::to_string).or_else(|| locate(conn, track)) {
            Some(path) => Ok((hash_of(conn, &path)?.or(track.hash.clone()), Some(path))),
            None => Ok((track.hash.clone(), None)),
        }
    };
    Ok(match op {
        Op::Play { track, started } => {
            let (hash, path) = file(track)?;
            // A track not here is found by its audio once it is (db::resolve).
            let Some(path) = path.or_else(|| hash.as_ref().map(|h| format!("sync:{h}"))) else {
                return Ok(false);
            };
            conn.execute(
                "INSERT OR IGNORE INTO plays (track_path, played_at, hash) VALUES (?1, ?2, ?3)",
                params![path, *started as i64, hash],
            )? > 0
        }
        Op::Scrobbled {
            service,
            device,
            seq,
        } => {
            conn.execute(
                "INSERT OR IGNORE INTO scrobbled (service, device, seq) VALUES (?1, ?2, ?3)",
                params![service, device, *seq as i64],
            )? > 0
        }
        Op::Name { playlist, name } => {
            let key = format!("name/{playlist}");
            if name.trim().is_empty()
                || tombstoned(conn, &format!("deleted/{playlist}"))?
                || !wins(conn, &key, stamp)?
            {
                return Ok(false);
            }
            set_stamp(conn, &key, stamp)?;
            conn.execute(
                "INSERT INTO playlists (id, name) VALUES (?1, ?2)
                 ON CONFLICT (id) DO UPDATE SET name = ?2",
                [playlist, name.trim()],
            )?;
            true
        }
        Op::Delete { playlist } => {
            let key = format!("deleted/{playlist}");
            if tombstoned(conn, &key)? {
                return Ok(false);
            }
            set_stamp(conn, &key, stamp)?;
            conn.execute("DELETE FROM playlist_items WHERE playlist = ?1", [playlist])?;
            conn.execute("DELETE FROM playlists WHERE id = ?1", [playlist])?;
            true
        }
        Op::Add {
            playlist,
            item,
            track,
            pos,
        } => {
            if tombstoned(conn, &format!("removed/{playlist}/{item}"))? {
                return Ok(false);
            }
            let (hash, path) = file(track)?;
            // Ops come causes first, so a playlist not here was deleted.
            conn.execute(
                "INSERT OR IGNORE INTO playlist_items (playlist, item, pos, file, hash)
                 SELECT ?1, ?2, ?3, ?4, ?5 WHERE EXISTS (SELECT 1 FROM playlists WHERE id = ?1)",
                params![playlist, item, pos, path.unwrap_or_default(), hash],
            )? > 0
        }
        Op::Remove { playlist, item } => {
            let key = format!("removed/{playlist}/{item}");
            if tombstoned(conn, &key)? {
                return Ok(false);
            }
            set_stamp(conn, &key, stamp)?;
            conn.execute(
                "DELETE FROM playlist_items WHERE playlist = ?1 AND item = ?2",
                [playlist, item],
            )? > 0
        }
        Op::Move {
            playlist,
            item,
            pos,
        } => {
            let key = format!("pos/{playlist}/{item}");
            if !wins(conn, &key, stamp)? {
                return Ok(false);
            }
            set_stamp(conn, &key, stamp)?;
            conn.execute(
                "UPDATE playlist_items SET pos = ?3 WHERE playlist = ?1 AND item = ?2",
                params![playlist, item, pos],
            )? > 0
        }
        Op::Favourite { track, .. } | Op::Rating { track, .. } => {
            if matches!(op, Op::Rating { rating: Some(r), .. } if !(1..=10).contains(r)) {
                return Ok(false);
            }
            let (hash, path) = file(track)?;
            // Marks are kept under the hash, else the path (ratings.rs).
            let Some(key) = hash.clone().or(path) else {
                return Ok(false);
            };
            let field = if matches!(op, Op::Favourite { .. }) {
                "favourite"
            } else {
                "rating"
            };
            let field = format!("{field}/{key}");
            if !wins(conn, &field, stamp)? {
                return Ok(false);
            }
            set_stamp(conn, &field, stamp)?;
            let old = crate::ratings::marks(conn, &key)?;
            crate::ratings::write(conn, &key, hash.as_deref(), marked(op, old))?
        }
    })
}

/// The marks `old` become with `op`.
pub(crate) fn marked(op: &Op, old: crate::ratings::Marks) -> crate::ratings::Marks {
    match op {
        Op::Favourite { on, .. } => crate::ratings::Marks {
            favourite: *on,
            ..old
        },
        Op::Rating { rating, .. } => crate::ratings::Marks {
            rating: *rating,
            ..old
        },
        _ => old,
    }
}

/// Logs what the library holds as `device`'s ops, the first time it syncs.
fn seed(conn: &Connection, device: &str) -> Result<()> {
    let strings = |sql: &str, args: &[&str]| -> Result<Vec<(String, String, Option<String>)>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    };
    // A file no longer here still has its audio, if it was hashed.
    let track = |path: &str, hash: Option<String>| -> Result<Option<Track>> {
        Ok(track_at(conn, path)?.or(hash.map(|hash| Track {
            hash: Some(hash),
            ..Default::default()
        })))
    };
    let playlists = strings("SELECT id, name, NULL FROM playlists ORDER BY rowid", &[])?;
    for (playlist, name, _) in playlists {
        record(
            conn,
            device,
            Op::Name {
                playlist: playlist.clone(),
                name,
            },
            None,
        )?;
        let items = strings(
            "SELECT item, file, hash FROM playlist_items WHERE playlist = ?1 ORDER BY pos, item",
            &[&playlist],
        )?;
        for (item, file, hash) in items {
            let pos: f64 = conn.query_row(
                "SELECT pos FROM playlist_items WHERE playlist = ?1 AND item = ?2",
                [&playlist, &item],
                |r| r.get(0),
            )?;
            if let Some(track) = track(&file, hash)? {
                let op = Op::Add {
                    playlist: playlist.clone(),
                    item,
                    track,
                    pos,
                };
                record(conn, device, op, Some(&file))?;
            }
        }
    }
    let mut stmt = conn.prepare("SELECT key, hash, favourite, rating FROM ratings")?;
    let marks = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, Option<u8>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (key, hash, favourite, rating) in marks {
        let here = hash.is_none().then_some(key.as_str());
        let Some(track) = track(&key, hash.clone())? else {
            continue;
        };
        if favourite {
            let op = Op::Favourite {
                track: track.clone(),
                on: true,
            };
            record(conn, device, op, here)?;
        }
        if rating.is_some() {
            record(conn, device, Op::Rating { track, rating }, here)?;
        }
    }
    let mut stmt =
        conn.prepare("SELECT track_path, played_at, hash FROM plays ORDER BY played_at")?;
    let plays = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (path, started, hash) in plays {
        if let Some(track) = track(&path, hash)? {
            let op = Op::Play {
                track,
                started: started as u64,
            };
            record(conn, device, op, Some(&path))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{LibraryQuery, QueryResult, TrackFilter};

    use super::*;

    const TITLES: [&str; 4] = ["A", "B", "C", "D"];

    /// An engine with the same four songs as the others, in its own folder.
    struct Engine {
        name: String,
        library: Library,
        dir: PathBuf,
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// What a user sees, which must match on every engine.
    #[derive(Debug, PartialEq)]
    struct Seen {
        playlists: Vec<(String, String, Vec<String>)>,
        plays: Vec<(String, u64)>,
        marks: Vec<(String, bool, Option<u8>)>,
    }

    fn engine(name: &str) -> Engine {
        let engine = unnamed(name);
        engine.library.set_device(name).unwrap();
        engine
    }

    /// An engine whose library doesn't know its key yet.
    fn unnamed(name: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!(
            "motif-sync-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        for (i, title) in TITLES.iter().enumerate() {
            library
                .db()
                .execute(
                    "INSERT INTO tracks (path, size, mtime, title, artist, album, duration_ms, audio_hash)
                     VALUES (?1, 1, 1, ?2, 'Band', 'Alpha', 1000, ?3)",
                    params![format!("/{name}/{title}.flac"), title, format!("h{i}")],
                )
                .unwrap();
        }
        Engine {
            name: name.into(),
            library,
            dir,
        }
    }

    impl Engine {
        fn paths(&self, titles: &[&str]) -> Vec<String> {
            titles
                .iter()
                .map(|t| format!("/{}/{t}.flac", self.name))
                .collect()
        }

        fn query(&self, query: LibraryQuery) -> QueryResult {
            self.library.query(query).unwrap()
        }

        fn create(&self, id: &str, name: &str, titles: &[&str]) {
            self.query(LibraryQuery::CreatePlaylist {
                id: id.into(),
                name: name.into(),
                paths: self.paths(titles),
            });
        }

        fn play(&self, title: &str, at: u64) -> bool {
            let track = TrackRef {
                source: "local".into(),
                id: self.paths(&[title]).remove(0),
                ..Default::default()
            };
            self.library.record_play(&track, at).unwrap()
        }

        fn playlists(&self) -> Vec<String> {
            let QueryResult::Playlists { playlists } = self.query(LibraryQuery::Playlists) else {
                panic!()
            };
            playlists.into_iter().map(|p| p.id).collect()
        }

        fn items(&self, playlist: &str) -> Vec<String> {
            let QueryResult::PlaylistTracks { entries } =
                self.query(LibraryQuery::PlaylistTracks {
                    id: playlist.into(),
                })
            else {
                panic!()
            };
            entries.into_iter().map(|e| e.item).collect()
        }

        fn seen(&self) -> Seen {
            let QueryResult::Playlists { playlists } = self.query(LibraryQuery::Playlists) else {
                panic!()
            };
            let playlists = playlists
                .into_iter()
                .map(|p| {
                    let QueryResult::PlaylistTracks { entries } =
                        self.query(LibraryQuery::PlaylistTracks { id: p.id.clone() })
                    else {
                        panic!()
                    };
                    let titles = entries
                        .into_iter()
                        .map(|e| e.track.track.title.unwrap())
                        .collect();
                    (p.id, p.name, titles)
                })
                .collect();
            let QueryResult::Played { plays, .. } = self.query(LibraryQuery::RecentlyPlayed {
                offset: 0,
                limit: 10_000,
            }) else {
                panic!()
            };
            let mut plays: Vec<_> = plays
                .into_iter()
                .map(|p| (p.track.track.title.unwrap(), p.played_at))
                .collect();
            plays.sort();
            let QueryResult::Tracks { tracks, .. } = self.query(LibraryQuery::Tracks {
                search: None,
                sort: vec![],
                filter: TrackFilter::default(),
                offset: 0,
                limit: 100,
            }) else {
                panic!()
            };
            let marks = tracks
                .into_iter()
                .map(|t| (t.track.title.unwrap(), t.favourite, t.rating))
                .collect();
            Seen {
                playlists,
                plays,
                marks,
            }
        }
    }

    /// Sends `to` what it lacks of `from`'s, a few ops at a time.
    fn give(from: &Engine, to: &Engine) {
        let mut theirs = to.library.versions().unwrap();
        loop {
            let page = from.library.ops_since(&theirs, 3).unwrap();
            if page.is_empty() {
                return;
            }
            for op in &page {
                theirs.insert(op.device.clone(), op.seq);
            }
            to.library.merge(page).unwrap();
        }
    }

    fn meet(a: &Engine, b: &Engine) {
        give(a, b);
        give(b, a);
    }

    #[test]
    fn changes_spread_through_an_engine_in_between() {
        let (phone, desktop, laptop) = (engine("phone"), engine("desktop"), engine("laptop"));
        phone.create("road", "Road", &["A", "B"]);
        assert!(phone.play("A", 100));
        phone
            .library
            .set_favourite(&phone.paths(&["C"]), true)
            .unwrap();
        laptop
            .library
            .set_rating(&laptop.paths(&["D"]), Some(8))
            .unwrap();
        meet(&phone, &desktop);
        meet(&desktop, &laptop);
        let seen = laptop.seen();
        assert_eq!(
            seen.playlists,
            [("road".into(), "Road".into(), vec!["A".into(), "B".into()])]
        );
        assert_eq!(seen.plays, [("A".into(), 100)]);
        assert_eq!(seen.marks[2], ("C".into(), true, None));
        assert_eq!(seen.marks[3], ("D".into(), false, Some(8)));
        meet(&phone, &desktop);
        assert_eq!(phone.seen(), seen);
        assert_eq!(desktop.seen(), seen);
    }

    #[test]
    fn a_deleted_playlist_stays_deleted_and_a_play_counts_once() {
        let (phone, desktop, laptop) = (engine("phone"), engine("desktop"), engine("laptop"));
        phone.create("p", "P", &["A"]);
        meet(&phone, &desktop);
        meet(&phone, &laptop);
        // At once: the phone deletes it, the desktop renames it and adds to it.
        phone.query(LibraryQuery::DeletePlaylist { id: "p".into() });
        desktop.query(LibraryQuery::RenamePlaylist {
            id: "p".into(),
            name: "Q".into(),
        });
        desktop.query(LibraryQuery::AddToPlaylist {
            id: "p".into(),
            op: "o".into(),
            paths: desktop.paths(&["B"]),
        });
        assert!(laptop.play("A", 50));
        // The laptop's play reaches the phone both ways.
        meet(&desktop, &laptop);
        meet(&phone, &laptop);
        meet(&phone, &desktop);
        let seen = phone.seen();
        assert!(seen.playlists.is_empty(), "{seen:?}");
        assert_eq!(seen.plays, [("A".into(), 50)]);
        assert_eq!(desktop.seen(), seen);
        assert_eq!(laptop.seen(), seen);
        // Creating it again (a client retrying) doesn't bring it back.
        desktop.create("p", "P", &["A"]);
        assert!(desktop.seen().playlists.is_empty());
    }

    #[test]
    fn what_was_there_before_sync_syncs_too() {
        let (phone, desktop) = (unnamed("phone"), engine("desktop"));
        phone.create("old", "Old", &["A"]);
        phone.play("B", 10);
        phone
            .library
            .set_rating(&phone.paths(&["C"]), Some(4))
            .unwrap();
        phone.library.set_device("phone").unwrap();
        let versions = phone.library.versions().unwrap();
        phone.library.set_device("phone").unwrap();
        assert_eq!(
            phone.library.versions().unwrap(),
            versions,
            "safe to repeat"
        );
        meet(&phone, &desktop);
        assert_eq!(desktop.seen(), phone.seen());
        assert_eq!(desktop.seen().plays, [("B".into(), 10)]);

        // Opened again: a change made before the key is known is still the phone's.
        let reopened = Library::open(&phone.dir).unwrap();
        assert!(
            reopened
                .record_play(
                    &TrackRef {
                        source: "local".into(),
                        id: "/phone/D.flac".into(),
                        ..Default::default()
                    },
                    20
                )
                .unwrap()
        );
        reopened.set_device("phone").unwrap();
        assert_eq!(reopened.versions().unwrap()["phone"], versions["phone"] + 1);
        meet(&phone, &desktop);
        assert_eq!(desktop.seen().plays, [("B".into(), 10), ("D".into(), 20)]);
    }

    #[test]
    fn an_interrupted_sync_resumes_and_gaps_are_refused() {
        let (phone, desktop) = (engine("phone"), engine("desktop"));
        for (i, title) in TITLES.iter().enumerate() {
            phone.play(title, i as u64);
        }
        let first = phone.library.ops_since(&Versions::new(), 2).unwrap();
        assert_eq!(first.len(), 2);
        desktop.library.merge(first.clone()).unwrap();
        desktop.library.merge(first).unwrap();
        assert_eq!(desktop.library.versions().unwrap()["phone"], 2);
        let mut rest = phone
            .library
            .ops_since(&desktop.library.versions().unwrap(), 10)
            .unwrap();
        rest.remove(0);
        assert!(desktop.library.merge(rest).is_err(), "op 3 is missing");
        meet(&phone, &desktop);
        assert_eq!(desktop.seen(), phone.seen());
        assert_eq!(desktop.seen().plays.len(), 4);
    }

    /// xorshift64: the same steps for the same seed.
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
        fn pick<T: Clone>(&mut self, from: &[T]) -> Option<T> {
            (!from.is_empty()).then(|| from[self.below(from.len())].clone())
        }
    }

    #[test]
    fn engines_converge_whatever_they_do_and_whenever_they_meet() {
        for seed in 1..=25u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let engines = [engine("a"), engine("b"), engine("c")];
            for step in 0..60 {
                let e = &engines[rng.below(3)];
                let title = TITLES[rng.below(4)];
                let playlist = rng.pick(&e.playlists());
                match rng.below(10) {
                    0 => {
                        let titles = &TITLES[..rng.below(3)];
                        e.create(&format!("p{step}"), &format!("n{step}"), titles);
                    }
                    1 => {
                        if let Some(id) = playlist {
                            let name = format!("r{step}");
                            e.query(LibraryQuery::RenamePlaylist { id, name });
                        }
                    }
                    2 => {
                        if let Some(id) = playlist {
                            e.query(LibraryQuery::DeletePlaylist { id });
                        }
                    }
                    3 => {
                        if let Some(id) = playlist {
                            let (op, paths) = (format!("o{step}"), e.paths(&[title]));
                            e.query(LibraryQuery::AddToPlaylist { id, op, paths });
                        }
                    }
                    4 => {
                        if let Some(id) = playlist {
                            let items = rng.pick(&e.items(&id)).into_iter().collect();
                            e.query(LibraryQuery::RemoveFromPlaylist { id, items });
                        }
                    }
                    5 => {
                        if let Some(id) = playlist {
                            let items = e.items(&id);
                            if let Some(item) = rng.pick(&items) {
                                let before = rng.pick(&items).filter(|_| rng.below(4) > 0);
                                e.query(LibraryQuery::MovePlaylistItem { id, item, before });
                            }
                        }
                    }
                    6 => {
                        e.play(title, rng.below(20) as u64);
                    }
                    7 => {
                        let on = rng.below(2) == 0;
                        e.library.set_favourite(&e.paths(&[title]), on).unwrap();
                    }
                    8 => {
                        let rating = Some(rng.below(11) as u8).filter(|&r| r > 0);
                        e.library.set_rating(&e.paths(&[title]), rating).unwrap();
                    }
                    _ => {
                        let (x, y) = (rng.below(3), rng.below(3));
                        meet(&engines[x], &engines[y]);
                    }
                }
            }
            meet(&engines[0], &engines[1]);
            meet(&engines[1], &engines[2]);
            meet(&engines[0], &engines[1]);
            let seen = engines[0].seen();
            assert_eq!(engines[1].seen(), seen, "seed {seed}");
            assert_eq!(engines[2].seen(), seen, "seed {seed}");
            meet(&engines[0], &engines[2]);
            assert_eq!(
                engines[2].seen(),
                seen,
                "seed {seed}: meeting again changes nothing"
            );
        }
    }

    #[test]
    fn a_scrobbled_play_is_marked_for_every_engine_and_service_separately() {
        let (a, b) = (engine("a"), engine("b"));
        let now = 10_000_000;
        a.play("A", now - 100);
        a.play("B", now - 15 * 86_400);
        meet(&a, &b);
        let listed =
            |e: &Engine, service| e.library.unscrobbled(service, now - 14 * 86_400).unwrap();
        let found = listed(&b, "lastfm");
        assert_eq!(
            found
                .iter()
                .map(|p| (p.track.title.clone().unwrap(), p.started))
                .collect::<Vec<_>>(),
            [("A".to_string(), now - 100)],
            "the old play is past what services take"
        );
        assert!(
            b.library
                .mark_scrobbled("lastfm", &found[0].device, found[0].seq)
                .unwrap()
        );
        assert!(
            !b.library
                .mark_scrobbled("lastfm", &found[0].device, found[0].seq)
                .unwrap(),
            "safe to repeat"
        );
        assert!(listed(&b, "lastfm").is_empty());
        assert_eq!(
            listed(&b, "listenbrainz").len(),
            1,
            "another service still lacks it"
        );
        meet(&a, &b);
        assert!(
            listed(&a, "lastfm").is_empty(),
            "the mark synced back to where it was played"
        );
        assert_eq!(a.seen(), b.seen(), "marks aren't something a user sees");
    }
}
