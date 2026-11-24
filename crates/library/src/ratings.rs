//! Favourites (a heart) and star ratings. A track's are stored under its
//! audio hash, so they follow the file when it moves and can match across
//! devices; until the scanner has hashed the file, under its path (the
//! `tracks_hashed` trigger in db.rs then moves them to the hash).

use protocol::TrackFilter;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    Error, Library, Result,
    sync::{self, Op, Track},
};

pub(crate) const SCHEMA: &str = "
    -- `key` is `hash`, or the track's path while it has none. `rating` counts
    -- half stars (1 to 10); a row with neither mark is deleted.
    CREATE TABLE IF NOT EXISTS ratings (
        key TEXT PRIMARY KEY,
        hash TEXT,
        favourite INTEGER NOT NULL DEFAULT 0,
        rating INTEGER
    );";

/// The `ratings.key` of the row of `tracks` a query is on.
macro_rules! key {
    () => {
        "coalesce(nullif(tracks.audio_hash, ''), tracks.path)"
    };
}

/// SQL for a track's favourite mark (0 or 1) and its half stars (NULL if unrated).
macro_rules! favourite {
    () => {
        concat!(
            "coalesce((SELECT favourite FROM ratings WHERE key = ",
            key!(),
            "), 0)"
        )
    };
}
macro_rules! rating {
    () => {
        concat!("(SELECT rating FROM ratings WHERE key = ", key!(), ")")
    };
}

pub(crate) const FAVOURITE: &str = favourite!();
pub(crate) const RATING: &str = rating!();

/// `WHERE` terms limiting tracks to what `filter` asks for (true if nothing).
pub(crate) fn filter(filter: &TrackFilter) -> String {
    let mut terms = vec!["1".to_string()];
    if filter.favourite {
        terms.push(format!("{FAVOURITE} = 1"));
    }
    if let Some(min) = filter.min_rating {
        terms.push(format!("coalesce({RATING}, 0) >= {}", min.min(10)));
    }
    terms.join(" AND ")
}

impl Library {
    /// Marks or unmarks the tracks at `paths` as favourites. Paths not in the
    /// library are left out. Safe to repeat: clients hear of it only when
    /// something changed.
    pub fn set_favourite(&self, paths: &[String], on: bool) -> Result<()> {
        self.mark(paths, |track| Op::Favourite { track, on })
    }

    /// Rates the tracks at `paths` from 1 to 10 half stars, or clears the
    /// rating with `None`.
    pub fn set_rating(&self, paths: &[String], rating: Option<u8>) -> Result<()> {
        if rating.is_some_and(|r| !(1..=10).contains(&r)) {
            return Err(Error::Other("a rating is 1 to 10 half stars".into()));
        }
        self.mark(paths, |track| Op::Rating { track, rating })
    }

    fn mark(&self, paths: &[String], op: impl Fn(Track) -> Op) -> Result<()> {
        let mut changed = false;
        {
            let conn = self.db();
            let device = self.device();
            let tx = conn.unchecked_transaction()?;
            for path in paths {
                let Some(track) = sync::track_at(&tx, path)? else {
                    continue;
                };
                let key = track.hash.clone().unwrap_or_else(|| path.clone());
                let (op, old) = (op(track), marks(&tx, &key)?);
                if sync::marked(&op, old) != old {
                    changed |= sync::record(&tx, &device, op, Some(path))?;
                }
            }
            tx.commit()?;
        }
        if changed {
            // Lists show the marks; the library itself is unchanged.
            self.shared.update(|s| s.ratings += 1);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Marks {
    pub favourite: bool,
    pub rating: Option<u8>,
}

/// The marks stored under `key`.
pub(crate) fn marks(conn: &Connection, key: &str) -> Result<Marks> {
    let marks = conn
        .query_row(
            "SELECT favourite, rating FROM ratings WHERE key = ?1",
            [key],
            |r| {
                Ok(Marks {
                    favourite: r.get(0)?,
                    rating: r.get(1)?,
                })
            },
        )
        .optional()?;
    Ok(marks.unwrap_or(Marks {
        favourite: false,
        rating: None,
    }))
}

/// Stores `new` under `key` (the audio `hash`, else the path); whether they changed.
pub(crate) fn write(conn: &Connection, key: &str, hash: Option<&str>, new: Marks) -> Result<bool> {
    if marks(conn, key)? == new {
        return Ok(false);
    }
    if !new.favourite && new.rating.is_none() {
        conn.execute("DELETE FROM ratings WHERE key = ?1", [key])?;
    } else {
        conn.execute(
            "INSERT INTO ratings (key, hash, favourite, rating) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (key) DO UPDATE SET favourite = ?3, rating = ?4",
            params![key, hash, new.favourite, new.rating],
        )?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{LibraryQuery, QueryResult, SortKey, SortRule};

    fn library() -> (Library, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "motif-ratings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        for (path, title) in [("/m/a.flac", "A"), ("/m/b.flac", "B"), ("/m/c.flac", "C")] {
            library
                .db()
                .execute(
                    "INSERT INTO tracks (path, size, mtime, title, artist, album, duration_ms)
                     VALUES (?1, 1, 1, ?2, 'Band', 'Alpha', 1000)",
                    params![path, title],
                )
                .unwrap();
        }
        (library, dir)
    }

    fn query(
        library: &Library,
        filter: TrackFilter,
        sort: Vec<SortRule>,
    ) -> Vec<(String, bool, Option<u8>)> {
        let QueryResult::Tracks { tracks, .. } = library
            .query(LibraryQuery::Tracks {
                search: None,
                sort,
                filter,
                offset: 0,
                limit: 10,
            })
            .unwrap()
        else {
            panic!()
        };
        tracks
            .into_iter()
            .map(|t| (t.track.title.unwrap(), t.favourite, t.rating))
            .collect()
    }

    fn all(library: &Library) -> Vec<(String, bool, Option<u8>)> {
        query(library, TrackFilter::default(), vec![])
    }

    fn one(path: &str) -> Vec<String> {
        vec![path.into()]
    }

    #[test]
    fn marks_show_on_tracks_and_setting_twice_changes_nothing() {
        let (library, dir) = library();
        let before = library.status();
        library.set_favourite(&one("/m/a.flac"), true).unwrap();
        library.set_rating(&one("/m/a.flac"), Some(7)).unwrap();
        let after = library.status();
        assert_eq!(after.ratings, before.ratings + 2, "clients refetch");
        assert_eq!(after.revision, before.revision);
        library.set_favourite(&one("/m/a.flac"), true).unwrap();
        library.set_rating(&one("/m/a.flac"), Some(7)).unwrap();
        assert_eq!(library.status(), after, "a repeat is not news");
        assert_eq!(
            all(&library),
            [
                ("A".into(), true, Some(7)),
                ("B".into(), false, None),
                ("C".into(), false, None)
            ]
        );
        library.set_favourite(&one("/m/a.flac"), false).unwrap();
        library.set_rating(&one("/m/a.flac"), None).unwrap();
        library.set_rating(&one("/m/a.flac"), None).unwrap();
        assert_eq!(all(&library)[0], ("A".into(), false, None));
        let rows: i64 = library
            .db()
            .query_row("SELECT count(*) FROM ratings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "nothing marked leaves no row");
        assert!(library.set_rating(&one("/m/a.flac"), Some(11)).is_err());
        assert!(library.set_rating(&one("/m/a.flac"), Some(0)).is_err());
        library
            .set_favourite(&one("/elsewhere.flac"), true)
            .unwrap();
        assert_eq!(
            library.status().ratings,
            after.ratings + 2,
            "unknown tracks are left out"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn marks_sort_and_filter() {
        let (library, dir) = library();
        library.set_rating(&one("/m/a.flac"), Some(3)).unwrap();
        library.set_rating(&one("/m/b.flac"), Some(9)).unwrap();
        library.set_favourite(&one("/m/c.flac"), true).unwrap();
        let rule = |key, descending| vec![SortRule { key, descending }];
        let titles = |rows: Vec<(String, bool, Option<u8>)>| {
            rows.into_iter().map(|r| r.0).collect::<Vec<_>>()
        };
        let by = |key, descending| {
            titles(query(
                &library,
                TrackFilter::default(),
                rule(key, descending),
            ))
        };
        assert_eq!(by(SortKey::Rating, true), ["B", "A", "C"]);
        assert_eq!(by(SortKey::Rating, false), ["C", "A", "B"], "unrated first");
        assert_eq!(by(SortKey::Favourite, true), ["C", "A", "B"]);
        let filtered = |favourite, min_rating| {
            titles(query(
                &library,
                TrackFilter {
                    favourite,
                    min_rating,
                },
                vec![],
            ))
        };
        assert_eq!(filtered(true, None), ["C"]);
        assert_eq!(filtered(false, Some(4)), ["B"]);
        assert_eq!(filtered(false, Some(1)), ["A", "B"]);
        assert_eq!(filtered(true, Some(1)), Vec::<String>::new());
        let QueryResult::Tracks { total, .. } = library
            .query(LibraryQuery::Tracks {
                search: None,
                sort: vec![],
                filter: TrackFilter {
                    favourite: true,
                    min_rating: None,
                },
                offset: 0,
                limit: 10,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(total, 1, "the count follows the filter");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn marks_follow_the_audio_hash_when_it_comes_and_when_the_file_moves() {
        let (library, dir) = library();
        library.set_favourite(&one("/m/a.flac"), true).unwrap();
        library.set_rating(&one("/m/a.flac"), Some(10)).unwrap();
        // The scanner hashes it: the marks move from the path to the hash.
        library
            .db()
            .execute(
                "UPDATE tracks SET audio_hash = 'h1' WHERE path = '/m/a.flac'",
                [],
            )
            .unwrap();
        let key: (String, String) = library
            .db()
            .query_row("SELECT key, hash FROM ratings", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(key, ("h1".into(), "h1".into()));
        assert_eq!(all(&library)[0], ("A".into(), true, Some(10)));
        // Found again under another name, the old file missing.
        library
            .db()
            .execute_batch(
                "UPDATE tracks SET missing = 1 WHERE path = '/m/a.flac';
                 INSERT INTO tracks (path, size, mtime, title, artist, album, duration_ms, audio_hash)
                 VALUES ('/m/moved.flac', 1, 1, 'A', 'Band', 'Alpha', 1000, 'h1');",
            )
            .unwrap();
        let moved: Vec<_> = all(&library).into_iter().filter(|r| r.0 == "A").collect();
        assert_eq!(moved.len(), 2);
        assert!(
            moved.iter().all(|r| *r == ("A".into(), true, Some(10))),
            "{moved:?}"
        );
        // Marking either file marks the audio, once.
        library.set_favourite(&one("/m/moved.flac"), false).unwrap();
        assert!(all(&library).iter().all(|r| !r.1));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
