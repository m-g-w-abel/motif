use std::path::Path;

use protocol::{SortKey, SortRule};
use rusqlite::{Connection, OptionalExtension, params};

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
        SortKey::Year => "coalesce(original_year, year)",
        SortKey::Added => "added",
        SortKey::Title => "lower(title)",
        SortKey::Genre => "lower(genre)",
        SortKey::Disc => "coalesce(disc_no, 1)",
        SortKey::Track => "coalesce(track_no, 0)",
        SortKey::Duration => "duration_ms",
        SortKey::Format => "lower(codec)",
        SortKey::Path => "path",
        SortKey::Favourite => crate::ratings::FAVOURITE,
        SortKey::Rating => concat!("coalesce(", rating!(), ", 0)"),
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

/// An album's cover, in a query grouping `tracks` by [`ALBUM_KEY`]: the first track's that
/// has one, in the order the album page lists its tracks (disc, track, path), so
/// the card and the page agree even when the tracks carry different covers.
pub const ALBUM_ART: &str = "(SELECT f.art FROM tracks f WHERE f.art IS NOT NULL AND f.shadowed = 0
         AND coalesce(f.album_artist, f.artist, '') || char(31) || coalesce(f.album, '')
             = coalesce(tracks.album_artist, tracks.artist, '') || char(31) || coalesce(tracks.album, '')
         ORDER BY coalesce(f.disc_no, 1), coalesce(f.track_no, 0), f.path LIMIT 1)";

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    // Versions 1–15 were upgraded step by step until every library in use was 16.
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if (1..16).contains(&version) {
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
            Some(format!(
                "{} is from an old version of Motif ({version}) that this one can't upgrade: delete it to start a new library",
                path.display()
            )),
        ));
    }
    // WAL lets queries read while the scanner writes.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(crate::history::SCHEMA)?;
    conn.execute_batch(crate::ratings::SCHEMA)?;
    conn.execute_batch(crate::sync::SCHEMA)?;
    conn.execute_batch(crate::catalog::SCHEMA)?;
    conn.execute_batch(crate::downloads::SCHEMA)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS roots (path TEXT PRIMARY KEY);
         CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS art (key TEXT PRIMARY KEY, path TEXT NOT NULL);
         -- Seek bar waveforms by audio hash (waveform.rs), made when first asked for.
         CREATE TABLE IF NOT EXISTS waveforms (hash TEXT PRIMARY KEY, levels BLOB NOT NULL);
         -- Loudness of files without ReplayGain tags by audio hash (loudness.rs),
         -- measured when about to play: integrated LUFS (NULL if silent), sample peak.
         CREATE TABLE IF NOT EXISTS loudness (hash TEXT PRIMARY KEY, lufs REAL, peak REAL NOT NULL);
         -- LRCLIB answers by audio hash (lyrics.rs): LRC and plain text, both
         -- NULL for a miss, and when it was asked (Unix seconds).
         CREATE TABLE IF NOT EXISTS lyrics (hash TEXT PRIMARY KEY, synced TEXT, plain TEXT, fetched INTEGER NOT NULL);
         -- Saved playlists (playlists.rs). Ids come from clients, so repeated
         -- commands find their earlier work. Entries name a track by path and,
         -- for when it moves, by audio hash.
         CREATE TABLE IF NOT EXISTS playlists (id TEXT PRIMARY KEY, name TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS playlist_items (
             playlist TEXT NOT NULL, item TEXT NOT NULL, pos INTEGER NOT NULL,
             file TEXT NOT NULL, hash TEXT,
             PRIMARY KEY (playlist, item)
         );
         CREATE INDEX IF NOT EXISTS playlist_items_file ON playlist_items (file);
         -- Files whose old audio hash (before version 15) is waiting for the
         -- scanner's new one, so what is keyed by it follows (see move_hash()).
         CREATE TABLE IF NOT EXISTS hash_moves (path TEXT PRIMARY KEY, old TEXT NOT NULL);
         CREATE TABLE IF NOT EXISTS tracks (
             id INTEGER PRIMARY KEY,
             path TEXT NOT NULL UNIQUE,
             size INTEGER NOT NULL,
             mtime INTEGER NOT NULL,
             title TEXT, artist TEXT, album TEXT, album_artist TEXT,
             track_no INTEGER, disc_no INTEGER, year INTEGER, original_year INTEGER, genre TEXT,
             duration_ms INTEGER, codec TEXT, sample_rate INTEGER, bit_depth INTEGER,
             channels INTEGER, bitrate INTEGER,
             musicbrainz_recording TEXT, isrc TEXT, audio_hash TEXT,
             art TEXT,
             rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL, rg_album_peak REAL,
             missing INTEGER NOT NULL DEFAULT 0,
             problem TEXT,
             added INTEGER,
             musicbrainz_album TEXT,
             -- '' for this engine's files, else the key of the linked engine
             -- whose catalog the row is (catalog.rs), with its audio on
             -- another row that wins (`shadowed`).
             device TEXT NOT NULL DEFAULT '',
             shadowed INTEGER NOT NULL DEFAULT 0,
             -- The catalog change that last touched this engine's row.
             version INTEGER
         );
         CREATE INDEX IF NOT EXISTS tracks_mb ON tracks (musicbrainz_recording);
         CREATE INDEX IF NOT EXISTS tracks_isrc ON tracks (isrc);
         CREATE INDEX IF NOT EXISTS tracks_hash ON tracks (audio_hash);
         CREATE INDEX IF NOT EXISTS tracks_title ON tracks (lower(title));
         -- An album's tracks by ALBUM_KEY (same text, or SQLite won't use it),
         -- looked up for album gain whenever the engine resolves a track.
         CREATE INDEX IF NOT EXISTS tracks_album ON tracks (
             coalesce(album_artist, artist, '') || char(31) || coalesce(album, '')
         );
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
",
    )?;
    upgrade(&conn)?;
    // Triggers are made anew each time, so older libraries get their latest body.
    let shadow = |hash: &str| {
        format!(
            "UPDATE tracks SET shadowed = {SHADOWED} WHERE audio_hash = {hash} AND shadowed IS NOT {SHADOWED};"
        )
    };
    let next = "UPDATE catalog_clock SET n = n + 1;";
    let now = "(SELECT n FROM catalog_clock)";
    let columns = crate::catalog::COLUMNS;
    conn.execute_batch(&format!(
        "CREATE INDEX IF NOT EXISTS tracks_device ON tracks (device);
         CREATE INDEX IF NOT EXISTS tracks_version ON tracks (version);
         INSERT INTO catalog_clock SELECT coalesce(max(version), 0) FROM tracks
         WHERE NOT EXISTS (SELECT 1 FROM catalog_clock);
         -- Only searched columns: the triggers below update others.
         DROP TRIGGER IF EXISTS tracks_au;
         CREATE TRIGGER tracks_au AFTER UPDATE OF title, artist, album, album_artist ON tracks BEGIN
             INSERT INTO tracks_fts (tracks_fts, rowid, title, artist, album, album_artist)
             VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
             INSERT INTO tracks_fts (rowid, title, artist, album, album_artist)
             VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
         END;
         -- Each change to this engine's tracks is numbered, for linked
         -- engines' copies of its catalog (catalog.rs).
         DROP TRIGGER IF EXISTS tracks_catalog_ai;
         CREATE TRIGGER tracks_catalog_ai AFTER INSERT ON tracks WHEN new.device = '' BEGIN
             {next} UPDATE tracks SET version = {now} WHERE id = new.id;
         END;
         DROP TRIGGER IF EXISTS tracks_catalog_au;
         CREATE TRIGGER tracks_catalog_au AFTER UPDATE OF {columns}, missing, problem ON tracks
         WHEN new.device = '' BEGIN
             {next} UPDATE tracks SET version = {now} WHERE id = new.id;
         END;
         DROP TRIGGER IF EXISTS tracks_catalog_ad;
         CREATE TRIGGER tracks_catalog_ad AFTER DELETE ON tracks WHEN old.device = '' BEGIN
             {next} INSERT OR REPLACE INTO catalog_gone (id, version) VALUES (old.id, {now});
         END;
         -- One row of each audio shows ([`SHADOWED`]).
         DROP TRIGGER IF EXISTS tracks_shadow_ai;
         CREATE TRIGGER tracks_shadow_ai AFTER INSERT ON tracks WHEN coalesce(new.audio_hash, '') != '' BEGIN
             {new}
         END;
         DROP TRIGGER IF EXISTS tracks_shadow_au;
         CREATE TRIGGER tracks_shadow_au AFTER UPDATE OF audio_hash, device ON tracks BEGIN
             {old} {new}
         END;
         DROP TRIGGER IF EXISTS tracks_shadow_ad;
         CREATE TRIGGER tracks_shadow_ad AFTER DELETE ON tracks WHEN coalesce(old.audio_hash, '') != '' BEGIN
             {old}
         END;",
        new = shadow("new.audio_hash"),
        old = shadow("old.audio_hash"),
    ))?;
    conn.execute_batch(
        "-- Entries naming a file get its audio hash once the scanner knows it, so
         -- they find the file again after it moves (see resolve()). Made anew
         -- each time, so older libraries get its latest body.
         DROP TRIGGER IF EXISTS tracks_hashed;
         CREATE TRIGGER tracks_hashed AFTER UPDATE OF audio_hash ON tracks
         WHEN new.audio_hash != '' BEGIN
             UPDATE playlist_items SET hash = new.audio_hash WHERE file = new.path AND hash IS NULL;
             UPDATE plays SET hash = new.audio_hash WHERE track_path = new.path AND hash IS NULL;
             -- shortcut: if the audio already has marks (another copy of it), these stay under the path.
             UPDATE OR IGNORE ratings SET key = new.audio_hash, hash = new.audio_hash
             WHERE key = new.path AND hash IS NULL;
             -- Their sync stamps follow (sync.rs).
             UPDATE OR IGNORE stamps SET key = 'favourite/' || new.audio_hash
             WHERE key = 'favourite/' || new.path;
             UPDATE OR IGNORE stamps SET key = 'rating/' || new.audio_hash
             WHERE key = 'rating/' || new.path;
         END;",
    )?;
    Ok(conn)
}

/// Whether a row of `tracks` is hidden behind another holding its audio:
/// this engine's own, else the linked engine with the smallest key.
/// Duplicates of one engine all show.
const SHADOWED: &str = "(audio_hash != '' AND EXISTS (
    SELECT 1 FROM tracks t WHERE t.audio_hash = tracks.audio_hash AND t.device < tracks.device))";

/// The library track an entry naming its file (`file`, a column) and its
/// audio (`hash`) stands for: its file, else a present file with the same
/// audio (it moved; the same title first, since tracks of silence or
/// duplicates share audio; the copy shown first), else its file marked missing.
pub fn resolve(file: &str, hash: &str) -> String {
    format!(
        "coalesce(
            (SELECT id FROM tracks WHERE path = {file} AND missing = 0),
            (SELECT id FROM tracks WHERE audio_hash = {hash} AND missing = 0
             ORDER BY shadowed, title IS (SELECT title FROM tracks WHERE path = {file}) DESC, id DESC LIMIT 1),
            (SELECT id FROM tracks WHERE path = {file}))"
    )
}

/// Bumped when the scanner reads files better, so rows it got wrong are
/// re-read at the next scan, or when the table changes.
const VERSION: i32 = 16;

fn upgrade(conn: &Connection) -> rusqlite::Result<()> {
    let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    // Steps for later versions go here, e.g. `if version < 17 { UPDATE tracks SET mtime = -1 … }`.
    if version < VERSION {
        conn.pragma_update(None, "user_version", VERSION)?;
    }
    Ok(())
}

/// Moves everything keyed by the audio hash `old` to `new`, once the file
/// noted in `hash_moves` is hashed again.
pub fn move_hash(conn: &Connection, path: &str, new: &str) -> rusqlite::Result<()> {
    let old: Option<String> = conn
        .query_row("SELECT old FROM hash_moves WHERE path = ?1", [path], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(old) = old else { return Ok(()) };
    for table in ["playlist_items", "plays", "waveforms", "loudness", "lyrics"] {
        // Keyed tables may already hold the new hash (a duplicate file): it wins.
        conn.execute(
            &format!("UPDATE OR REPLACE {table} SET hash = ?1 WHERE hash = ?2"),
            [new, &old],
        )?;
    }
    conn.execute("DELETE FROM hash_moves WHERE path = ?1", [path])?;
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
/// Other copies of audio shown once ([`SHADOWED`]) are left out too.
pub const SEARCH: &str = "(shadowed = 0 AND (?1 IS NULL OR id IN (SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1)))";

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
    conn.query_row(
        "SELECT count(*) FROM tracks WHERE device = ''",
        params![],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_lookups_use_the_index() {
        let file = std::env::temp_dir().join(format!("motif-db-{}.db", std::process::id()));
        let conn = open(&file).unwrap();
        let plan: String = conn
            .query_row(
                &format!("EXPLAIN QUERY PLAN SELECT path FROM tracks WHERE {ALBUM_KEY} = ?1"),
                ["x"],
                |r| r.get(3),
            )
            .unwrap();
        assert!(plan.contains("tracks_album"), "{plan}");
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
    }

    #[test]
    fn an_album_shows_its_first_tracks_cover() {
        let file = std::env::temp_dir().join(format!("motif-dbart-{}.db", std::process::id()));
        let conn = open(&file).unwrap();
        // "b" sorts after "a" by name, but its track number is first; the first track has no cover.
        for (path, track, art) in [
            ("/a", 3, Some("zzz")),
            ("/b", 2, Some("mmm")),
            ("/c", 1, None),
            ("/d", 4, Some("aaa")),
        ] {
            conn.execute(
                "INSERT INTO tracks (path, size, mtime, album, track_no, art) VALUES (?1, 0, 0, 'Gemini', ?2, ?3)",
                params![path, track, art],
            )
            .unwrap();
        }
        // A hidden duplicate (another device's copy) never gives the cover, as the page doesn't list it.
        conn.execute(
            "INSERT INTO tracks (path, size, mtime, album, track_no, art, shadowed) VALUES ('/e', 0, 0, 'Gemini', 0, 'hidden', 1)",
            [],
        )
        .unwrap();
        let art: Option<String> = conn
            .query_row(
                &format!(
                    "SELECT {ALBUM_KEY} AS k, {ALBUM_ART} FROM tracks WHERE shadowed = 0 GROUP BY k"
                ),
                [],
                |r| r.get(1),
            )
            .unwrap();
        // max(art) would say "zzz".
        assert_eq!(art.as_deref(), Some("mmm"));
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
    }

    #[test]
    fn a_library_from_before_version_16_is_refused_and_a_new_one_opens() {
        let file = std::env::temp_dir().join(format!("motif-db15-{}.db", std::process::id()));
        let conn = open(&file).unwrap();
        conn.pragma_update(None, "user_version", 15).unwrap();
        drop(conn);
        let old = open(&file).map(|_| ()).unwrap_err().to_string();
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
        assert!(old.contains("delete it"), "{old}");
        let conn = open(&file).unwrap();
        let version: i32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
        assert_eq!(version, VERSION);
    }

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
        assert_eq!(
            order(&rules),
            "ORDER BY coalesce(original_year, year) DESC, lower(title), path"
        );
        assert_eq!(
            album_order(&rules),
            "ORDER BY max(coalesce(original_year, year)) DESC, min(lower(title)), k"
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
            SortKey::Favourite,
            SortKey::Rating,
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
