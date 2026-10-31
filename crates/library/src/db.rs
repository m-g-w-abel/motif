use std::path::Path;

use protocol::{SortKey, SortRule};
use rusqlite::{Connection, params};

/// The order lists use until the user picks one: album artist, album, disc, track.
const DEFAULT_SORT: [SortRule; 4] = [
    SortRule {
        key: SortKey::AlbumArtist,
        descending: false,
    },
    SortRule {
        key: SortKey::Album,
        descending: false,
    },
    SortRule {
        key: SortKey::Disc,
        descending: false,
    },
    SortRule {
        key: SortKey::Track,
        descending: false,
    },
];

/// The SQL a sort key orders by. Text sorts case-insensitively.
fn sort_expr(key: SortKey) -> &'static str {
    match key {
        SortKey::AlbumArtist => "lower(coalesce(album_artist, artist, ''))",
        SortKey::Artist => "lower(coalesce(artist, ''))",
        SortKey::Album => "lower(coalesce(album, ''))",
        SortKey::Year => "year",
        SortKey::Added => "added",
        SortKey::Title => "lower(title)",
        SortKey::Genre => "lower(genre)",
        SortKey::Disc => "coalesce(disc_no, 1)",
        SortKey::Track => "coalesce(track_no, 0)",
        SortKey::Duration => "duration_ms",
        SortKey::Format => "lower(codec)",
        SortKey::Path => "path",
    }
}

/// `ORDER BY` for tracks: each rule in turn (the default if there are none),
/// then path, so no two rows tie and pages never overlap.
pub fn order(rules: &[SortRule]) -> String {
    order_by(rules, false)
}

/// `ORDER BY` for albums grouped as `k` ([`ALBUM_KEY`]): each rule by its
/// lowest value in the album (highest when descending), then the album key.
pub fn album_order(rules: &[SortRule]) -> String {
    order_by(rules, true)
}

fn order_by(rules: &[SortRule], albums: bool) -> String {
    let rules = if rules.is_empty() {
        &DEFAULT_SORT[..]
    } else {
        rules
    };
    let mut terms: Vec<String> = rules
        .iter()
        .map(|r| {
            let e = sort_expr(r.key);
            match (albums, r.descending) {
                (false, false) => e.to_string(),
                (false, true) => format!("{e} DESC"),
                (true, false) => format!("min({e})"),
                (true, true) => format!("max({e}) DESC"),
            }
        })
        .collect();
    terms.push(if albums { "k" } else { "path" }.into());
    format!("ORDER BY {}", terms.join(", "))
}

/// Groups tracks into albums: album artist (or artist) plus album title,
/// joined by a unit separator.
pub const ALBUM_KEY: &str = "coalesce(album_artist, artist, '') || char(31) || coalesce(album, '')";

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // WAL lets queries read while the scanner writes.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS roots (path TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS art (key TEXT PRIMARY KEY, path TEXT NOT NULL);
         -- Seek bar waveforms by audio hash (waveform.rs), made when first asked for.
         CREATE TABLE IF NOT EXISTS waveforms (hash TEXT PRIMARY KEY, levels BLOB NOT NULL);
         -- Loudness of files without ReplayGain tags by audio hash (loudness.rs),
         -- measured when about to play: integrated LUFS (NULL if silent), sample peak.
         CREATE TABLE IF NOT EXISTS loudness (hash TEXT PRIMARY KEY, lufs REAL, peak REAL NOT NULL);
         CREATE TABLE IF NOT EXISTS tracks (
             id INTEGER PRIMARY KEY,
             path TEXT NOT NULL UNIQUE,
             size INTEGER NOT NULL,
             mtime INTEGER NOT NULL,
             title TEXT, artist TEXT, album TEXT, album_artist TEXT,
             track_no INTEGER, disc_no INTEGER, year INTEGER, genre TEXT,
             duration_ms INTEGER, codec TEXT, sample_rate INTEGER, bit_depth INTEGER,
             channels INTEGER, bitrate INTEGER,
             musicbrainz_recording TEXT, isrc TEXT, audio_hash TEXT,
             art TEXT,
             rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL, rg_album_peak REAL,
             missing INTEGER NOT NULL DEFAULT 0,
             problem TEXT,
             added INTEGER
         );
         CREATE INDEX IF NOT EXISTS tracks_mb ON tracks (musicbrainz_recording);
         CREATE INDEX IF NOT EXISTS tracks_isrc ON tracks (isrc);
         CREATE INDEX IF NOT EXISTS tracks_hash ON tracks (audio_hash);
         CREATE INDEX IF NOT EXISTS tracks_title ON tracks (lower(title));
         -- The default order (see order()): pages of a big library come from
         -- the index instead of sorting every track for each page.
         CREATE INDEX IF NOT EXISTS tracks_order ON tracks (
             lower(coalesce(album_artist, artist, '')), lower(coalesce(album, '')),
             coalesce(disc_no, 1), coalesce(track_no, 0), path
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS tracks_fts USING fts5(
             title, artist, album, album_artist, content='tracks', content_rowid='id'
         );
         CREATE TRIGGER IF NOT EXISTS tracks_ai AFTER INSERT ON tracks BEGIN
             INSERT INTO tracks_fts (rowid, title, artist, album, album_artist)
             VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_ad AFTER DELETE ON tracks BEGIN
             INSERT INTO tracks_fts (tracks_fts, rowid, title, artist, album, album_artist)
             VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
         END;
         CREATE TRIGGER IF NOT EXISTS tracks_au AFTER UPDATE ON tracks BEGIN
             INSERT INTO tracks_fts (tracks_fts, rowid, title, artist, album, album_artist)
             VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
             INSERT INTO tracks_fts (rowid, title, artist, album, album_artist)
             VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
         END;",
    )?;
    upgrade(&conn)?;
    Ok(conn)
}

/// Bumped when the scanner reads files better, so rows it got wrong are
/// re-read at the next scan, or when the table changes.
const VERSION: i32 = 7;

fn upgrade(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let has = |column: &str| {
        conn.prepare("SELECT 1 FROM pragma_table_info('tracks') WHERE name = ?1")
            .and_then(|mut s| s.exists([column]))
    };
    if version < 2 {
        // Version 2 reads files lofty can't parse with Symphonia instead.
        conn.execute("UPDATE tracks SET mtime = -1 WHERE duration_ms IS NULL", [])?;
    }
    if version < 3 {
        // Version 3 keeps tracks whose files disappear (marked missing) and
        // notes why damaged files can't be played.
        if !has("missing")? {
            conn.execute(
                "ALTER TABLE tracks ADD COLUMN missing INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        if !has("problem")? {
            conn.execute("ALTER TABLE tracks ADD COLUMN problem TEXT", [])?;
        }
    }
    if version < 4 && !has("added")? {
        // Version 4 notes when each track was added; the file's modification
        // time is the best guess for tracks already in the library.
        conn.execute("ALTER TABLE tracks ADD COLUMN added INTEGER", [])?;
        conn.execute("UPDATE tracks SET added = max(mtime, 0)", [])?;
    }
    if version < 5 {
        // Version 5 shows a folder image over embedded art when it is at
        // least as large: re-read tracks showing embedded art (cached as
        // `<key>.<ext>`; folder images keep their own names).
        conn.execute(
            "UPDATE tracks SET mtime = -1 WHERE art IN (SELECT key FROM art WHERE path GLOB '*' || key || '.*')",
            [],
        )?;
    }
    if version < 6 {
        // Version 6 gives ties to embedded art again. A folder image can
        // share its key with an embedded copy, so re-read every track with art.
        conn.execute("UPDATE tracks SET mtime = -1 WHERE art IS NOT NULL", [])?;
    }
    if version < 7 {
        // Version 7 reads ReplayGain tags: add the columns and re-read every file.
        for column in [
            "rg_track_gain",
            "rg_track_peak",
            "rg_album_gain",
            "rg_album_peak",
        ] {
            if !has(column)? {
                conn.execute(&format!("ALTER TABLE tracks ADD COLUMN {column} REAL"), [])?;
            }
        }
        conn.execute("UPDATE tracks SET mtime = -1", [])?;
    }
    if version < VERSION {
        conn.pragma_update(None, "user_version", VERSION)?;
    }
    Ok(())
}

/// Turns what the user typed into an FTS5 query: every word must match the
/// start of a word in title, artist or album.
pub fn fts_query(search: Option<&str>) -> Option<String> {
    let words: Vec<String> = search?
        .split_whitespace()
        .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// `WHERE` clause limiting tracks to a search (bound as `?1`, may be NULL).
pub const SEARCH: &str =
    "(?1 IS NULL OR id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1))";

/// `WHERE` clause for tracks inside the root folder bound as `?1`. The
/// separator matters: `/music` must not match `/music2/…`.
#[cfg(not(windows))]
pub const UNDER_ROOT: &str = "substr(path, 1, length(?1) + 1) = ?1 || '/'";
#[cfg(windows)]
pub const UNDER_ROOT: &str = "substr(path, 1, length(?1) + 1) = ?1 || '\\'";

/// Whether changes made outside Motif are picked up as they happen (on by default).
pub fn watching(conn: &Connection) -> bool {
    conn.query_row("SELECT value FROM settings WHERE key = 'watch'", [], |r| {
        r.get::<_, String>(0)
    })
    .map_or(true, |v| v != "off")
}

pub fn set_watching(conn: &Connection, on: bool) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('watch', ?1) ON CONFLICT (key) DO UPDATE SET value = ?1",
        [if on { "on" } else { "off" }],
    )?;
    Ok(())
}

pub fn roots(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM roots ORDER BY path")?;
    stmt.query_map([], |row| row.get(0))?.collect()
}

pub fn track_count(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("SELECT count(*) FROM tracks", params![], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rule(key: SortKey, descending: bool) -> SortRule {
        SortRule { key, descending }
    }

    #[test]
    fn no_rules_give_the_default_order() {
        assert_eq!(
            order(&[]),
            "ORDER BY lower(coalesce(album_artist, artist, '')), lower(coalesce(album, '')), \
             coalesce(disc_no, 1), coalesce(track_no, 0), path"
        );
        assert_eq!(order(&[]), order(&DEFAULT_SORT));
    }

    #[test]
    fn rules_become_tiers_in_order() {
        let rules = [rule(SortKey::Year, true), rule(SortKey::Title, false)];
        assert_eq!(order(&rules), "ORDER BY year DESC, lower(title), path");
        assert_eq!(
            album_order(&rules),
            "ORDER BY max(year) DESC, min(lower(title)), k"
        );
        assert_eq!(
            album_order(&[]),
            "ORDER BY min(lower(coalesce(album_artist, artist, ''))), min(lower(coalesce(album, ''))), \
             min(coalesce(disc_no, 1)), min(coalesce(track_no, 0)), k"
        );
    }

    #[test]
    fn every_key_orders_a_real_query() {
        let dir = std::env::temp_dir().join(format!("motif-order-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir.join("library.db")).unwrap();
        let keys = [
            SortKey::AlbumArtist,
            SortKey::Artist,
            SortKey::Album,
            SortKey::Year,
            SortKey::Added,
            SortKey::Title,
            SortKey::Genre,
            SortKey::Disc,
            SortKey::Track,
            SortKey::Duration,
            SortKey::Format,
            SortKey::Path,
        ];
        let rules: Vec<_> = keys.iter().map(|&k| rule(k, false)).collect();
        conn.prepare(&format!("SELECT id FROM tracks {}", order(&rules)))
            .unwrap();
        conn.prepare(&format!(
            "SELECT {ALBUM_KEY} AS k FROM tracks GROUP BY k {}",
            album_order(&rules)
        ))
        .unwrap();
        drop(conn);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_default_order_uses_its_index() {
        let dir = std::env::temp_dir().join(format!("motif-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open(&dir.join("library.db")).unwrap();
        let plan: String = conn
            .query_row(
                &format!(
                    "EXPLAIN QUERY PLAN SELECT id FROM tracks WHERE {SEARCH} {} LIMIT 200",
                    order(&[])
                ),
                [None::<String>],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("tracks_order"), "{plan}");
        drop(conn);
        let _ = std::fs::remove_dir_all(dir);
    }
}
