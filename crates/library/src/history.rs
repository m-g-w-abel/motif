//! Play history: one row per listen that counted as played (see the
//! integrations crate's scrobble rule), newest first for Recently Played.
//! Rows are only ever added, each with its time, so devices can merge their
//! histories later.

use protocol::{PlayedTrack, QueryResult, TrackRef};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Library, Result, TRACK_COLUMNS, db, library_album, library_track};

pub(crate) const SCHEMA: &str = "
    -- A listen of the track at track_path that started at played_at (Unix
    -- seconds). The key makes recording the same listen twice harmless.
    CREATE TABLE IF NOT EXISTS plays (
        track_path TEXT NOT NULL,
        played_at INTEGER NOT NULL,
        PRIMARY KEY (track_path, played_at)
    );
    CREATE INDEX IF NOT EXISTS plays_at ON plays (played_at);";

impl Library {
    /// Notes that `track` was played, starting at `started_at` (Unix
    /// seconds). Tracks not in the library are left out; returns whether
    /// one was added. Safe to repeat.
    pub fn record_play(&self, track: &TrackRef, started_at: u64) -> Result<bool> {
        let in_library = |conn: &Connection| {
            conn.query_row("SELECT 1 FROM tracks WHERE path = ?1", [&track.id], |_| {
                Ok(())
            })
            .optional()
            .map(|found| found.is_some())
        };
        let path = if track.source == "local" && in_library(&self.db())? {
            track.id.clone()
        } else if let Some(path) = self.find(track) {
            path.to_string_lossy().into_owned()
        } else {
            return Ok(false);
        };
        let added = self.db().execute(
            "INSERT OR IGNORE INTO plays (track_path, played_at) VALUES (?1, ?2)",
            params![path, started_at as i64],
        )?;
        if added > 0 {
            // Clients refetch Recently Played; the library itself is unchanged.
            self.shared.update(|s| s.plays += 1);
        }
        Ok(added > 0)
    }

    pub(crate) fn recently_played(&self, offset: u32, limit: u32) -> Result<QueryResult> {
        let conn = self.db();
        let total = conn.query_row(
            "SELECT count(*) FROM plays JOIN tracks ON path = track_path",
            [],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {TRACK_COLUMNS}, played_at FROM plays JOIN tracks ON path = track_path
             ORDER BY played_at DESC, track_path LIMIT ?1 OFFSET ?2"
        ))?;
        let plays = stmt
            .query_map(params![limit, offset], |r| {
                Ok(PlayedTrack {
                    track: library_track(r)?,
                    played_at: r.get::<_, i64>(23)? as u64,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(QueryResult::Played {
            total,
            offset,
            plays,
        })
    }

    pub(crate) fn recently_played_albums(&self, offset: u32, limit: u32) -> Result<QueryResult> {
        let conn = self.db();
        let total = conn.query_row(
            &format!(
                "SELECT count(DISTINCT {}) FROM plays JOIN tracks ON path = track_path",
                db::ALBUM_KEY
            ),
            [],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {key} AS k, coalesce(album, ''), coalesce(album_artist, artist, ''), min(coalesce(original_year, year)),
                    count(*), sum(coalesce(duration_ms, 0)), max(art)
             FROM tracks JOIN (
                 SELECT {key} AS played_key, max(played_at) AS last
                 FROM plays JOIN tracks ON path = track_path GROUP BY played_key
             ) ON played_key = {key}
             GROUP BY k ORDER BY last DESC, k LIMIT ?1 OFFSET ?2",
            key = db::ALBUM_KEY
        ))?;
        let albums = stmt
            .query_map(params![limit, offset], library_album)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(QueryResult::Albums {
            total,
            offset,
            albums,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "motif-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        for (path, title, album) in [
            ("/m/a1.flac", "A1", "Alpha"),
            ("/m/a2.flac", "A2", "Alpha"),
            ("/m/b1.flac", "B1", "Beta"),
        ] {
            library
                .db()
                .execute(
                    "INSERT INTO tracks (path, size, mtime, title, artist, album, duration_ms)
                     VALUES (?1, 1, 1, ?2, 'Band', ?3, 1000)",
                    params![path, title, album],
                )
                .unwrap();
        }
        (library, dir)
    }

    fn local(path: &str) -> TrackRef {
        TrackRef {
            source: "local".into(),
            id: path.into(),
            ..Default::default()
        }
    }

    fn titles(result: QueryResult) -> Vec<(String, u64)> {
        let QueryResult::Played { plays, .. } = result else {
            panic!("{result:?}")
        };
        plays
            .into_iter()
            .map(|p| (p.track.track.title.unwrap(), p.played_at))
            .collect()
    }

    #[test]
    fn plays_are_listed_newest_first_and_survive_a_restart() {
        let (library, dir) = library();
        assert!(library.record_play(&local("/m/a1.flac"), 100).unwrap());
        assert!(library.record_play(&local("/m/b1.flac"), 300).unwrap());
        assert!(library.record_play(&local("/m/a1.flac"), 200).unwrap());
        let expected = vec![
            ("B1".to_string(), 300),
            ("A1".into(), 200),
            ("A1".into(), 100),
        ];
        assert_eq!(titles(library.recently_played(0, 10).unwrap()), expected);
        assert_eq!(
            titles(library.recently_played(1, 1).unwrap()),
            vec![("A1".to_string(), 200)],
            "paging"
        );
        drop(library);
        let again = Library::open(&dir).unwrap();
        assert_eq!(titles(again.recently_played(0, 10).unwrap()), expected);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recording_the_same_play_twice_adds_it_once_and_unknown_tracks_are_left_out() {
        let (library, dir) = library();
        let before = library.status();
        assert!(library.record_play(&local("/m/a1.flac"), 100).unwrap());
        let after = library.status();
        assert_eq!(
            after.plays,
            before.plays + 1,
            "clients refetch what was played"
        );
        assert_eq!(
            after.revision, before.revision,
            "the library itself didn't change, so live contexts aren't re-read"
        );
        assert!(!library.record_play(&local("/m/a1.flac"), 100).unwrap());
        assert_eq!(library.status(), after, "nothing new, nothing to refetch");
        assert!(!library.record_play(&local("/elsewhere.flac"), 100).unwrap());
        let QueryResult::Played { total, .. } = library.recently_played(0, 10).unwrap() else {
            panic!()
        };
        assert_eq!(total, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn albums_come_newest_play_first() {
        let (library, dir) = library();
        library.record_play(&local("/m/a1.flac"), 100).unwrap();
        library.record_play(&local("/m/b1.flac"), 200).unwrap();
        library.record_play(&local("/m/a2.flac"), 300).unwrap();
        library.record_play(&local("/m/a1.flac"), 400).unwrap();
        let QueryResult::Albums { total, albums, .. } =
            library.recently_played_albums(0, 10).unwrap()
        else {
            panic!()
        };
        assert_eq!(total, 2);
        let names: Vec<_> = albums.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(
            names,
            ["Alpha", "Beta"],
            "Alpha was played last, once per play"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
