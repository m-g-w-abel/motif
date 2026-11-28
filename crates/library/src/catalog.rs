//! Catalog sharing between linked engines (#241): each engine's own tracks,
//! offered to the engines it is linked with, which keep them as rows of
//! `tracks` with the owner's key in `device`. A catalog belongs to its owner,
//! so it isn't merged: the owner numbers every change to its tracks
//! (`tracks.version`, `catalog_gone` for deletions, triggers in db.rs) and
//! sends the changes after the last one the other has seen ([`Seen`]); a
//! new library (another `epoch`) starts again from nothing. The same audio
//! on several engines is one track: every other copy is `shadowed`, a local
//! copy winning, then the smallest key.

use std::path::PathBuf;

use protocol::TrackRef;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::{Error, Library, Result};

/// What an engine holds of another's catalog.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Seen {
    /// Which library of the owner's it came from; `None` if nothing yet.
    pub epoch: Option<String>,
    /// The owner's last change applied.
    pub version: u64,
}

/// A track as its owner offers it: no path.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct CatalogTrack {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub original_year: Option<u32>,
    pub genre: Option<String>,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    pub bitrate: Option<u32>,
    pub musicbrainz_recording: Option<String>,
    pub musicbrainz_album: Option<String>,
    pub isrc: Option<String>,
    pub audio_hash: Option<String>,
    pub art: Option<String>,
    pub rg_track_gain: Option<f64>,
    pub rg_track_peak: Option<f64>,
    pub rg_album_gain: Option<f64>,
    pub rg_album_peak: Option<f64>,
    pub added: Option<i64>,
}

/// Change `version` of the owner's catalog: its track `id` is now `track`,
/// or gone (`None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Change {
    pub version: u64,
    pub id: i64,
    pub track: Option<CatalogTrack>,
}

/// Changes of the owner's library `epoch` after its change `from`, oldest
/// first. Pages are refused if they come from an older library than the
/// one held, or don't follow on from what is held.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub epoch: String,
    pub from: u64,
    pub changes: Vec<Change>,
}

pub(crate) const SCHEMA: &str = "
    -- The catalogs of linked engines held here: which library of theirs
    -- (`epoch`) and its last change applied.
    CREATE TABLE IF NOT EXISTS catalogs (device TEXT PRIMARY KEY, epoch TEXT NOT NULL, version INTEGER NOT NULL);
    -- This engine's deleted tracks, by the change that deleted them.
    CREATE TABLE IF NOT EXISTS catalog_gone (id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS catalog_gone_version ON catalog_gone (version);
    -- The number of this engine's last catalog change (one row).
    CREATE TABLE IF NOT EXISTS catalog_clock (n INTEGER NOT NULL);";

/// The columns of `tracks` a catalog carries, in [`CatalogTrack`]'s order.
pub(crate) const COLUMNS: &str = "title, artist, album, album_artist, track_no, disc_no, year, \
    original_year, genre, duration_ms, codec, sample_rate, bit_depth, channels, bitrate, \
    musicbrainz_recording, musicbrainz_album, isrc, audio_hash, art, rg_track_gain, rg_track_peak, \
    rg_album_gain, rg_album_peak, added";

impl Library {
    /// What this engine holds of `device`'s catalog.
    pub fn catalog_seen(&self, device: &str) -> Result<Seen> {
        seen(&self.db(), device)
    }

    /// Up to `limit` changes to this engine's catalog after `seen` (all of
    /// it if `seen` is of another library).
    pub fn catalog_page(&self, seen: &Seen, limit: u32) -> Result<Page> {
        let conn = self.db();
        let epoch = epoch(&conn)?;
        let since = match &seen.epoch {
            Some(e) if *e == epoch => seen.version as i64,
            _ => 0,
        };
        // Missing and damaged files are gone as far as other engines go.
        let mut stmt = conn.prepare(&format!(
            "SELECT version, id, missing = 1 OR problem IS NOT NULL, {COLUMNS} FROM tracks
             WHERE device = '' AND version > ?1 ORDER BY version LIMIT ?2"
        ))?;
        let mut changes = stmt
            .query_map(params![since, limit], |r| {
                let gone: bool = r.get(2)?;
                Ok(Change {
                    version: r.get::<_, i64>(0)? as u64,
                    id: r.get(1)?,
                    track: if gone { None } else { Some(track(r, 3)?) },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut stmt = conn.prepare(
            "SELECT version, id FROM catalog_gone WHERE version > ?1 ORDER BY version LIMIT ?2",
        )?;
        for gone in stmt.query_map(params![since, limit], |r| {
            Ok(Change {
                version: r.get::<_, i64>(0)? as u64,
                id: r.get(1)?,
                track: None,
            })
        })? {
            changes.push(gone?);
        }
        changes.sort_by_key(|c| c.version);
        changes.truncate(limit as usize);
        Ok(Page {
            epoch,
            from: since as u64,
            changes,
        })
    }

    /// Applies a page of `device`'s catalog. Changes already applied are
    /// skipped, so it is safe to repeat and an interrupted exchange resumes.
    pub fn merge_catalog(&self, device: &str, mut page: Page) -> Result<()> {
        if device.is_empty() {
            return Err(Error::Other("a catalog needs its engine".into()));
        }
        page.changes.sort_by_key(|c| c.version);
        let mut changed = false;
        {
            let conn = self.db();
            let tx = crate::db::write_tx(&conn)?;
            let seen = seen(&tx, device)?;
            let refuse = |why: &str| Err(Error::Other(format!("catalog of {device}: {why}")));
            let mut version = match &seen.epoch {
                // Held up from before the owner's library was made anew.
                Some(held) if page.epoch < *held => return refuse("from an older library"),
                Some(held) if page.epoch == *held && page.from > seen.version => {
                    return refuse("changes missing before these");
                }
                Some(held) if page.epoch == *held => seen.version,
                _ if page.from != 0 => return refuse("a new library's changes from the middle"),
                _ => {
                    // Another library of the owner's: what it had is no more.
                    changed |= tx.execute("DELETE FROM tracks WHERE device = ?1", [device])? > 0;
                    0
                }
            };
            let sets = COLUMNS
                .split(", ")
                .map(|c| format!("{c} = excluded.{c}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut upsert = tx.prepare(&format!(
                "INSERT INTO tracks (path, size, mtime, device, {COLUMNS})
                 VALUES (?1, 0, 0, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                         ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)
                 ON CONFLICT (path) DO UPDATE SET {sets}"
            ))?;
            for change in page.changes {
                if change.version <= version {
                    continue;
                }
                version = change.version;
                changed = true;
                // Rows stand for the owner's track ids; its paths never leave it.
                let path = format!("remote:{device}:{}", change.id);
                let Some(t) = change.track else {
                    tx.execute("DELETE FROM tracks WHERE path = ?1", [&path])?;
                    continue;
                };
                let hash = t.audio_hash.clone().filter(|h| !h.is_empty());
                upsert.execute(params![
                    path,
                    device,
                    t.title,
                    t.artist,
                    t.album,
                    t.album_artist,
                    t.track_no,
                    t.disc_no,
                    t.year,
                    t.original_year,
                    t.genre,
                    t.duration_ms.map(|d| d as i64),
                    t.codec,
                    t.sample_rate,
                    t.bit_depth,
                    t.channels,
                    t.bitrate,
                    t.musicbrainz_recording,
                    t.musicbrainz_album,
                    t.isrc,
                    t.audio_hash,
                    t.art,
                    t.rg_track_gain,
                    t.rg_track_peak,
                    t.rg_album_gain,
                    t.rg_album_peak,
                    t.added,
                ])?;
                if let Some(hash) = hash {
                    crate::downloads::source_hashed(&tx, &path, &hash)?;
                }
            }
            drop(upsert);
            tx.execute(
                "INSERT OR REPLACE INTO catalogs (device, epoch, version) VALUES (?1, ?2, ?3)",
                params![device, page.epoch, version as i64],
            )?;
            tx.commit()?;
        }
        if changed {
            self.shared.update(|s| s.revision += 1);
        }
        Ok(())
    }

    /// Drops the catalogs of engines other than `devices` (no longer linked).
    pub fn keep_catalogs(&self, devices: &[String]) -> Result<()> {
        let devices = serde_json::to_string(devices).map_err(|e| Error::Other(e.to_string()))?;
        let dropped = {
            let conn = self.db();
            let tx = crate::db::write_tx(&conn)?;
            let dropped = tx.execute(
                "DELETE FROM tracks WHERE device != ''
                 AND device NOT IN (SELECT value FROM json_each(?1))",
                [&devices],
            )?;
            tx.execute(
                "DELETE FROM catalogs WHERE device NOT IN (SELECT value FROM json_each(?1))",
                [&devices],
            )?;
            tx.commit()?;
            dropped
        };
        if dropped > 0 {
            self.shared.update(|s| s.revision += 1);
        }
        Ok(())
    }

    /// The linked engines holding `track` (a catalog row's own reference,
    /// or the same audio), each with its id for the track there.
    pub fn copies(&self, track: &TrackRef) -> Vec<(String, i64)> {
        let conn = self.db();
        let hash = track.keys.audio_hash.as_deref().unwrap_or("");
        let found = conn
            .prepare(
                "SELECT path FROM tracks WHERE device != '' AND missing = 0
                 AND (path = ?1 OR audio_hash = nullif(?2, '')) ORDER BY device",
            )
            .and_then(|mut stmt| {
                stmt.query_map(params![track.id, hash], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            });
        let found = found.unwrap_or_else(|err| {
            tracing::warn!(%err, "can't look for linked copies");
            Vec::new()
        });
        // A reference from elsewhere, matched by its other keys or tags.
        let located = crate::sync::locate(&conn, &track.into());
        let mut copies: Vec<_> = found
            .iter()
            .chain(&located)
            .filter_map(|path| {
                let (device, id) = path.strip_prefix("remote:")?.split_once(':')?;
                Some((device.to_string(), id.parse().ok()?))
            })
            .collect();
        copies.sort();
        copies.dedup();
        copies
    }

    /// The linked engine to ask for artwork `key`: one whose catalog shows
    /// it, when it isn't stored here yet. `None` for a key not in the
    /// right form too: it names a file here (#266).
    pub fn art_owner(&self, key: &str) -> Option<String> {
        if !valid_art_key(key) || self.art_source(key).is_ok() {
            return None;
        }
        self.db()
            .query_row(
                "SELECT device FROM tracks WHERE art = ?1 AND device != '' LIMIT 1",
                [key],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Stores artwork `key` fetched from its owner like a folder image, so
    /// [`Self::art`] makes thumbnails of it as of any other.
    pub fn save_remote_art(&self, key: &str, bytes: &[u8]) -> Result<()> {
        if !valid_art_key(key) {
            return Err(Error::Other("not an artwork key".into()));
        }
        image::guess_format(bytes).map_err(|e| Error::Other(e.to_string()))?;
        let file = self.shared.art_dir.join(format!("{key}.img"));
        let part = file.with_extension("img.part");
        std::fs::write(&part, bytes)?;
        std::fs::rename(&part, &file)?;
        self.db().execute(
            "INSERT OR REPLACE INTO art (key, path) VALUES (?1, ?2)",
            params![key, file.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Where the seek bar waveform of a linked engine's track `path` is
    /// to be asked for: the engine, its id for the track and, if the
    /// catalog has it, the audio hash it is cached by. `None` for local tracks.
    pub fn remote_waveform(&self, path: &str) -> Option<(String, i64, Option<String>)> {
        let (device, id) = path.strip_prefix("remote:")?.split_once(':')?;
        let hash = self
            .db()
            .query_row(
                "SELECT audio_hash FROM tracks WHERE path = ?1",
                [path],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .flatten()
            .filter(|h| !h.is_empty());
        Some((device.to_string(), id.parse().ok()?, hash))
    }

    /// The waveform cached for audio hash `hash`.
    pub fn cached_waveform(&self, hash: &str) -> Option<Vec<u8>> {
        self.db()
            .query_row(
                "SELECT levels FROM waveforms WHERE hash = ?1",
                [hash],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    /// Caches a waveform fetched from the engine holding the track.
    pub fn store_waveform(&self, hash: &str, levels: &[u8]) {
        let (hash, levels) = (hash.to_string(), levels.to_vec());
        self.write_aside("waveform", move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO waveforms (hash, levels) VALUES (?1, ?2)",
                params![hash, levels],
            )
        });
    }

    /// The file of this engine's own track `id`, if it is here: what a
    /// linked engine may read ([`Self::copies`]).
    pub fn own_file(&self, id: i64) -> Option<OwnFile> {
        self.db()
            .query_row(
                "SELECT path, codec, bitrate, audio_hash FROM tracks
                 WHERE id = ?1 AND device = '' AND missing = 0 AND problem IS NULL",
                [id],
                |r| {
                    Ok(OwnFile {
                        path: PathBuf::from(r.get::<_, String>(0)?),
                        codec: r.get(1)?,
                        bitrate: r.get(2)?,
                        audio_hash: r.get(3)?,
                    })
                },
            )
            .optional()
            .ok()
            .flatten()
    }
}

/// One of this engine's own files, as a linked engine is sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnFile {
    pub path: PathBuf,
    pub codec: Option<String>,
    /// kb/s.
    pub bitrate: Option<u32>,
    /// What a copy encoded from it carries, so it dedupes with it.
    pub audio_hash: Option<String>,
}

/// Artwork keys are 32 hex digits ([`crate::scan::art_key`]): the name of a
/// file in the art folder, which a peer's key must never reach out of.
pub fn valid_art_key(key: &str) -> bool {
    key.len() == 32 && key.bytes().all(|b| b.is_ascii_hexdigit())
}

fn seen(conn: &Connection, device: &str) -> Result<Seen> {
    let seen = conn
        .query_row(
            "SELECT epoch, version FROM catalogs WHERE device = ?1",
            [device],
            |r| {
                Ok(Seen {
                    epoch: Some(r.get(0)?),
                    version: r.get::<_, i64>(1)? as u64,
                })
            },
        )
        .optional()?;
    Ok(seen.unwrap_or_default())
}

/// This library's epoch, made the first time it is asked for: its changes
/// count from 1 again if the library is made anew. It is when that was, in
/// fixed-width hex, so a later library's sorts after an earlier one's.
// shortcut: a library made anew while the clock is behind the old one's
// first share is refused by engines holding the old; key it on a counter
// kept outside library.db if that ever happens.
fn epoch(conn: &Connection) -> Result<String> {
    let made = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('catalog_epoch', ?1)",
        [format!("{made:032x}")],
    )?;
    Ok(conn.query_row(
        "SELECT value FROM settings WHERE key = 'catalog_epoch'",
        [],
        |r| r.get(0),
    )?)
}

/// The [`COLUMNS`] from column `at` on.
fn track(r: &Row<'_>, at: usize) -> rusqlite::Result<CatalogTrack> {
    Ok(CatalogTrack {
        title: r.get(at)?,
        artist: r.get(at + 1)?,
        album: r.get(at + 2)?,
        album_artist: r.get(at + 3)?,
        track_no: r.get(at + 4)?,
        disc_no: r.get(at + 5)?,
        year: r.get(at + 6)?,
        original_year: r.get(at + 7)?,
        genre: r.get(at + 8)?,
        duration_ms: r.get::<_, Option<i64>>(at + 9)?.map(|d| d as u64),
        codec: r.get(at + 10)?,
        sample_rate: r.get(at + 11)?,
        bit_depth: r.get(at + 12)?,
        channels: r.get(at + 13)?,
        bitrate: r.get(at + 14)?,
        musicbrainz_recording: r.get(at + 15)?,
        musicbrainz_album: r.get(at + 16)?,
        isrc: r.get(at + 17)?,
        audio_hash: r.get(at + 18)?,
        art: r.get(at + 19)?,
        rg_track_gain: r.get(at + 20)?,
        rg_track_peak: r.get(at + 21)?,
        rg_album_gain: r.get(at + 22)?,
        rg_album_peak: r.get(at + 23)?,
        added: r.get(at + 24)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use protocol::{Command, LibraryQuery, LibraryTrack, QueryResult, TrackFilter, TrackRef};

    use super::*;

    struct Engine {
        key: String,
        library: Library,
        dir: PathBuf,
    }

    impl Drop for Engine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn dir(key: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "motif-catalog-{key}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An engine with tracks (title, audio hash) of album "Alpha".
    fn engine(key: &str, tracks: &[(&str, Option<&str>)]) -> Engine {
        let dir = dir(key);
        let library = Library::open(&dir).unwrap();
        library.set_device(key).unwrap();
        let engine = Engine {
            key: key.into(),
            library,
            dir,
        };
        for &(title, hash) in tracks {
            engine.add(title, hash);
        }
        engine
    }

    impl Engine {
        fn path(&self, title: &str) -> String {
            format!("/{}/{title}.flac", self.key)
        }

        fn add(&self, title: &str, hash: Option<&str>) {
            self.library
                .db()
                .execute(
                    "INSERT INTO tracks (path, size, mtime, title, artist, album, duration_ms, audio_hash)
                     VALUES (?1, 1, 1, ?2, 'Band', 'Alpha', 1000, ?3)",
                    params![self.path(title), title, hash],
                )
                .unwrap();
        }

        fn sql(&self, sql: &str) {
            self.library.db().execute_batch(sql).unwrap();
        }

        fn tracks(&self) -> Vec<LibraryTrack> {
            let QueryResult::Tracks { tracks, total, .. } = self
                .library
                .query(LibraryQuery::Tracks {
                    search: None,
                    sort: vec![],
                    filter: TrackFilter::default(),
                    offset: 0,
                    limit: 100,
                })
                .unwrap()
            else {
                panic!()
            };
            assert_eq!(total as usize, tracks.len());
            tracks
        }

        /// Titles shown, each with the engines holding it.
        fn shown(&self) -> Vec<(String, Vec<String>)> {
            let mut shown: Vec<_> = self
                .tracks()
                .into_iter()
                .map(|t| (t.track.title.unwrap(), t.devices))
                .collect();
            shown.sort();
            shown
        }
    }

    /// Sends `to` what it lacks of `from`'s catalog, two changes at a time.
    fn give(from: &Engine, to: &Engine) {
        let mut seen = to.library.catalog_seen(&from.key).unwrap();
        loop {
            let page = from.library.catalog_page(&seen, 2).unwrap();
            let Some(last) = page.changes.last() else {
                return;
            };
            seen = Seen {
                epoch: Some(page.epoch.clone()),
                version: last.version,
            };
            to.library.merge_catalog(&from.key, page).unwrap();
        }
    }

    fn meet(a: &Engine, b: &Engine) {
        give(a, b);
        give(b, a);
    }

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    fn id(engine: &Engine, title: &str) -> i64 {
        engine
            .library
            .db()
            .query_row(
                "SELECT id FROM tracks WHERE path = ?1",
                [engine.path(title)],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn reference(id: &str, hash: Option<&str>) -> TrackRef {
        TrackRef {
            source: "local".into(),
            id: id.into(),
            keys: protocol::TrackKeys {
                audio_hash: hash.map(str::to_string),
                ..Default::default()
            },
            title: None,
            artist: None,
            album: None,
            duration_ms: None,
        }
    }

    #[test]
    fn linked_copies_are_found_by_reference_and_files_served_only_by_own_id() {
        let a = engine("a", &[("A1", Some("h1")), ("A2", Some("h2"))]);
        let c = engine("c", &[("C1", Some("h1"))]);
        let b = engine("b", &[("B1", Some("h1"))]);
        give(&a, &b);
        give(&c, &b);
        let (a1, a2, c1) = (id(&a, "A1"), id(&a, "A2"), id(&c, "C1"));
        // By the remote row's own path, and by audio hash from a local one.
        assert_eq!(
            b.library
                .copies(&reference(&format!("remote:a:{a2}"), None)),
            [("a".to_string(), a2)]
        );
        assert_eq!(
            b.library.copies(&reference(&b.path("B1"), Some("h1"))),
            [("a".to_string(), a1), ("c".to_string(), c1)]
        );
        assert!(
            b.library
                .copies(&reference("/elsewhere.flac", None))
                .is_empty()
        );
        // Only this engine's own, present files, by id.
        assert_eq!(
            a.library.own_file(a1).map(|f| f.path),
            Some(PathBuf::from(a.path("A1")))
        );
        let remote_row: i64 = b
            .library
            .db()
            .query_row(
                "SELECT id FROM tracks WHERE device = 'a' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b.library.own_file(remote_row), None);
        a.sql(&format!("UPDATE tracks SET missing = 1 WHERE id = {a1}"));
        assert_eq!(a.library.own_file(a1), None);
    }

    #[test]
    fn linked_engines_see_each_others_tracks_and_the_same_audio_once() {
        let a = engine("a", &[("A1", Some("h1")), ("A2", Some("h2"))]);
        let b = engine("b", &[("B1", Some("h1")), ("B3", Some("h3")), ("B4", None)]);
        meet(&a, &b);
        assert_eq!(
            a.shown(),
            [
                ("A1".into(), keys(&["a", "b"])),
                ("A2".into(), keys(&["a"])),
                ("B3".into(), keys(&["b"])),
                ("B4".into(), keys(&["b"])),
            ],
            "the local copy of h1 wins"
        );
        assert_eq!(
            b.shown(),
            [
                ("A2".into(), keys(&["a"])),
                ("B1".into(), keys(&["a", "b"])),
                ("B3".into(), keys(&["b"])),
                ("B4".into(), keys(&["b"])),
            ]
        );
        let remote = a
            .tracks()
            .into_iter()
            .find(|t| t.track.title.as_deref() == Some("B3"))
            .unwrap();
        assert!(
            !remote.track.id.contains("/b/"),
            "paths stay home: {}",
            remote.track.id
        );
        let QueryResult::Albums { albums, .. } = a
            .library
            .query(LibraryQuery::Albums {
                search: None,
                artist: None,
                sort: vec![],
                offset: 0,
                limit: 10,
                filter: TrackFilter::default(),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(albums[0].tracks, 4);
        let QueryResult::Tracks { total, .. } = a
            .library
            .query(LibraryQuery::Tracks {
                search: Some("b3".into()),
                sort: vec![],
                filter: TrackFilter::default(),
                offset: 0,
                limit: 10,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(total, 1, "search finds other engines' tracks");
        let QueryResult::Folders { folders, .. } = a
            .library
            .query(LibraryQuery::Folders {
                search: None,
                offset: 0,
                limit: 10,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(folders.len(), 1, "folders are this engine's: {folders:?}");
        let root = a.dir.join("music");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.to_string_lossy().into_owned();
        a.library.query(LibraryQuery::AddRoot { path }).unwrap();
        assert_eq!(a.library.status().tracks, 2, "this engine's tracks");

        // The local copy goes: the other engine's shows instead.
        a.sql("DELETE FROM tracks WHERE path = '/a/A1.flac'");
        assert!(
            a.shown().contains(&("B1".into(), keys(&["b"]))),
            "{:?}",
            a.shown()
        );
    }

    #[test]
    fn changes_follow_and_applying_them_again_changes_nothing() {
        let a = engine("a", &[("A1", Some("h1")), ("A2", Some("h2")), ("A3", None)]);
        let b = engine("b", &[]);
        give(&a, &b);
        let first = a.library.catalog_page(&Seen::default(), 100).unwrap();
        a.sql(
            "UPDATE tracks SET title = 'Renamed' WHERE path = '/a/A1.flac';
             UPDATE tracks SET missing = 1 WHERE path = '/a/A2.flac';
             DELETE FROM tracks WHERE path = '/a/A3.flac';",
        );
        a.add("A4", Some("h4"));
        give(&a, &b);
        let shown = b.shown();
        assert_eq!(
            shown,
            [
                ("A4".into(), keys(&["a"])),
                ("Renamed".into(), keys(&["a"])),
            ]
        );
        // An old page again, or the whole catalog again: nothing changes.
        b.library.merge_catalog("a", first).unwrap();
        let all = a.library.catalog_page(&Seen::default(), 100).unwrap();
        b.library.merge_catalog("a", all).unwrap();
        assert_eq!(b.shown(), shown);
        let seen = b.library.catalog_seen("a").unwrap();
        assert!(
            a.library
                .catalog_page(&seen, 100)
                .unwrap()
                .changes
                .is_empty(),
            "up to date"
        );
    }

    #[test]
    fn a_new_library_on_the_owner_replaces_its_old_catalog() {
        let a = engine("a", &[("Old", Some("h1"))]);
        let b = engine("b", &[]);
        give(&a, &b);
        // The same engine with a fresh library: its changes count from 1 again.
        let fresh = engine("a", &[("New", Some("h2"))]);
        give(&fresh, &b);
        assert_eq!(b.shown(), [("New".into(), keys(&["a"]))]);
        drop(a);
    }

    #[test]
    fn a_page_from_an_older_library_or_after_a_gap_is_refused() {
        let old = engine("a", &[("Old", Some("h1"))]);
        let b = engine("b", &[]);
        give(&old, &b);
        // Held up in a session from before the owner's library was made anew.
        let stale = old.library.catalog_page(&Seen::default(), 10).unwrap();
        let fresh = engine("a", &[("New", Some("h2"))]);
        give(&fresh, &b);
        assert!(b.library.merge_catalog("a", stale).is_err());
        assert_eq!(b.shown(), [("New".into(), keys(&["a"]))]);
        // A new library's changes must start from the beginning.
        let newest = engine("a", &[("X", None), ("Y", None)]);
        let tail = newest
            .library
            .catalog_page(
                &Seen {
                    epoch: None,
                    version: 0,
                },
                10,
            )
            .unwrap();
        let mut partial = tail.clone();
        partial.from = 1;
        partial.changes.remove(0);
        assert!(b.library.merge_catalog("a", partial).is_err());
        assert_eq!(b.shown(), [("New".into(), keys(&["a"]))]);
        // Changes after ones not held yet would leave a hole.
        fresh.add("Later", Some("h3"));
        fresh.add("Latest", Some("h4"));
        let mut seen = b.library.catalog_seen("a").unwrap();
        seen.version += 1;
        let gap = fresh.library.catalog_page(&seen, 10).unwrap();
        assert!(b.library.merge_catalog("a", gap).is_err());
        give(&fresh, &b);
        assert_eq!(b.shown().len(), 3);
        drop((old, newest));
    }

    #[test]
    fn unlinking_drops_an_engines_tracks() {
        let a = engine("a", &[("A1", Some("h1"))]);
        let b = engine("b", &[("B1", Some("h1")), ("B2", None)]);
        let c = engine("c", &[("C1", None)]);
        meet(&a, &b);
        meet(&a, &c);
        a.library.keep_catalogs(&keys(&["c"])).unwrap();
        assert_eq!(
            a.shown(),
            [("A1".into(), keys(&["a"])), ("C1".into(), keys(&["c"]))]
        );
        a.library.keep_catalogs(&keys(&["c"])).unwrap();
        assert_eq!(a.library.catalog_seen("b").unwrap(), Seen::default());
    }

    #[test]
    fn the_this_device_filter_leaves_out_other_engines_tracks() {
        let a = engine("a", &[("A1", Some("h1"))]);
        // B3 is on album Alpha with A1, but only on b.
        let b = engine("b", &[("B1", Some("h1")), ("B2", None), ("B3", Some("h3"))]);
        b.sql("UPDATE tracks SET album = 'Beta', artist = 'Other' WHERE title = 'B2'");
        meet(&a, &b);
        let here = TrackFilter {
            this_device: true,
            ..TrackFilter::default()
        };
        let query = |q| a.library.query(q).unwrap();
        let QueryResult::Tracks { total, tracks, .. } = query(LibraryQuery::Tracks {
            search: None,
            sort: vec![],
            filter: here,
            offset: 0,
            limit: 10,
        }) else {
            panic!()
        };
        assert_eq!((total, tracks[0].track.title.as_deref()), (1, Some("A1")));
        let QueryResult::Groups { groups } = query(LibraryQuery::Groups {
            search: None,
            sort: vec![],
            filter: here,
        }) else {
            panic!()
        };
        assert_eq!(groups.len(), 1, "{groups:?}");
        let QueryResult::Albums { total, .. } = query(LibraryQuery::Albums {
            search: None,
            artist: None,
            sort: vec![],
            filter: here,
            offset: 0,
            limit: 10,
        }) else {
            panic!()
        };
        assert_eq!(total, 1);
        let QueryResult::Artists { artists, .. } = query(LibraryQuery::Artists {
            search: None,
            album_artists: false,
            filter: here,
            offset: 0,
            limit: 10,
        }) else {
            panic!()
        };
        assert_eq!(artists.len(), 1, "{artists:?}");
        let Command::SetContext { tracks, .. } = a
            .library
            .selection(&LibraryQuery::Play {
                search: None,
                sort: vec![],
                filter: here,
                start: 0,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(tracks.len(), 1);
        // An album's page and play agree with its card.
        let alpha = "Band\u{1f}Alpha".to_string();
        let QueryResult::Tracks { tracks, .. } = query(LibraryQuery::AlbumTracks {
            albums: vec![alpha.clone()],
            sort: vec![],
            filter: here,
        }) else {
            panic!()
        };
        assert_eq!(tracks.len(), 1, "{tracks:?}");
        let Command::SetContext { tracks, .. } = a
            .library
            .selection(&LibraryQuery::PlayAlbum {
                album: alpha.clone(),
                sort: vec![],
                start: None,
                filter: here,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(tracks.len(), 1);
        let QueryResult::Position { index } = query(LibraryQuery::AlbumPosition {
            search: None,
            sort: vec![],
            album: "Other\u{1f}Beta".into(),
            filter: here,
        }) else {
            panic!()
        };
        assert_eq!(index, None, "Beta isn't here");
        // Without it, both engines' music shows.
        let QueryResult::Albums { total, .. } = query(LibraryQuery::Albums {
            search: None,
            artist: None,
            sort: vec![],
            filter: TrackFilter::default(),
            offset: 0,
            limit: 10,
        }) else {
            panic!()
        };
        assert_eq!(total, 2);
    }

    /// Query times with a big catalog from another engine (PLAN.md, #241):
    /// `cargo test -p library catalog::tests::timings -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timings() {
        use std::time::Instant;
        let run = |label: &str, local: usize, remote: usize| {
            let a = engine("timing", &[]);
            {
                let conn = a.library.db();
                let tx = conn.unchecked_transaction().unwrap();
                for i in 0..local {
                    tx.execute(
                        "INSERT INTO tracks (path, size, mtime, title, artist, album, track_no, duration_ms, audio_hash)
                         VALUES (?1, 1, 1, ?2, ?3, ?4, ?5, 200000, ?6)",
                        params![format!("/m/{i}.flac"), format!("Song {i}"), format!("Artist {}", i % 1000),
                                format!("Album {}", i / 10), (i % 10) as u32, format!("h{i}")],
                    )
                    .unwrap();
                }
                tx.commit().unwrap();
            }
            // Half of the local tracks are on the other engine too.
            let changes: Vec<_> = (0..remote)
                .map(|i| {
                    let n = i + local / 2;
                    Change {
                        version: i as u64 + 1,
                        id: i as i64,
                        track: Some(CatalogTrack {
                            title: Some(format!("Song {n}")),
                            artist: Some(format!("Artist {}", n % 1000)),
                            album: Some(format!("Album {}", n / 10)),
                            track_no: Some((n % 10) as u32),
                            duration_ms: Some(200_000),
                            audio_hash: Some(format!("h{n}")),
                            ..Default::default()
                        }),
                    }
                })
                .collect();
            let started = Instant::now();
            for page in changes.chunks(1000) {
                let page = Page {
                    epoch: "e".into(),
                    from: 0,
                    changes: page.to_vec(),
                };
                a.library.merge_catalog("b", page).unwrap();
            }
            let merged = started.elapsed();
            let time = |query: LibraryQuery| {
                let started = Instant::now();
                a.library.query(query).unwrap();
                started.elapsed()
            };
            let tracks = |search: Option<&str>| LibraryQuery::Tracks {
                search: search.map(str::to_string),
                sort: vec![],
                filter: TrackFilter::default(),
                offset: 20_000,
                limit: 200,
            };
            eprintln!(
                "{label}: merge {merged:?}; tracks page {:?}; search {:?}; albums {:?}; artists {:?}; groups {:?}",
                time(tracks(None)),
                time(tracks(Some("song 4"))),
                time(LibraryQuery::Albums {
                    search: None,
                    artist: None,
                    sort: vec![],
                    offset: 2000,
                    limit: 200,
                    filter: TrackFilter::default(),
                }),
                time(LibraryQuery::Artists {
                    search: None,
                    album_artists: true,
                    offset: 200,
                    limit: 200,
                    filter: TrackFilter::default(),
                }),
                time(LibraryQuery::Groups {
                    search: None,
                    sort: vec![],
                    filter: TrackFilter::default(),
                }),
            );
        };
        run("58k local", 58_000, 0);
        run("8k local + 50k remote", 8_000, 50_000);
    }
}

#[cfg(test)]
mod remote_art_tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    fn png() -> Vec<u8> {
        let mut out = Vec::new();
        image::RgbImage::new(4, 4)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    fn library() -> (Library, std::path::PathBuf) {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("motif-remote-art-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        library
            .db()
            .execute(
                "INSERT INTO tracks (path, size, mtime, device, title, art, audio_hash)
                 VALUES ('remote:a:7', 0, 0, 'a', 'T', ?1, 'h')",
                [KEY],
            )
            .unwrap();
        (library, dir)
    }

    #[test]
    fn a_linked_engines_cover_is_fetched_once_then_served_like_local_art() {
        let (library, dir) = library();
        assert!(library.art_image(KEY, None).is_err());
        assert_eq!(library.art_owner(KEY).as_deref(), Some("a"));
        library.save_remote_art(KEY, &png()).unwrap();
        assert_eq!(library.art_owner(KEY), None);
        assert_eq!(library.art_image(KEY, None).unwrap().1, "image/png");
        assert!(library.art_image(KEY, Some(2)).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn keys_that_are_not_hashes_never_name_files() {
        let (library, dir) = library();
        assert!(library.save_remote_art("../x", &png()).is_err());
        assert!(library.save_remote_art(KEY, b"not an image").is_err());
        assert_eq!(library.art_owner("../x"), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_linked_engines_waveform_is_found_by_its_hash_once_stored() {
        let (library, dir) = library();
        assert_eq!(
            library.remote_waveform("remote:a:7"),
            Some(("a".into(), 7, Some("h".into())))
        );
        assert_eq!(library.remote_waveform("/music/a.flac"), None);
        assert_eq!(library.cached_waveform("h"), None);
        library.store_waveform("h", &[1, 2, 3]);
        let until = std::time::Instant::now();
        while library.cached_waveform("h").is_none() && until.elapsed().as_secs() < 5 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(library.cached_waveform("h"), Some(vec![1, 2, 3]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
