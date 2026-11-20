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
    conn.execute_batch(crate::history::SCHEMA)?;
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
             musicbrainz_album TEXT
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
         CREATE TRIGGER IF NOT EXISTS tracks_au AFTER UPDATE ON tracks BEGIN
             INSERT INTO tracks_fts (tracks_fts, rowid, title, artist, album, album_artist)
             VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist);
             INSERT INTO tracks_fts (rowid, title, artist, album, album_artist)
             VALUES (new.id, new.title, new.artist, new.album, new.album_artist);
         END;",
    )?;
    upgrade(&conn, &path.with_file_name("art"))?;
    // After the upgrade, which adds `plays.hash`.
    conn.execute_batch(
        "-- Entries naming a file get its audio hash once the scanner knows it, so
         -- they find the file again after it moves (see resolve()).
         CREATE TRIGGER IF NOT EXISTS tracks_hashed AFTER UPDATE OF audio_hash ON tracks
         WHEN new.audio_hash != '' BEGIN
             UPDATE playlist_items SET hash = new.audio_hash WHERE file = new.path AND hash IS NULL;
             UPDATE plays SET hash = new.audio_hash WHERE track_path = new.path AND hash IS NULL;
         END;",
    )?;
    Ok(conn)
}

/// The library track an entry naming its file (`file`, a column) and its
/// audio (`hash`) stands for: its file, else a present file with the same
/// audio (it moved; the same title first, since tracks of silence or
/// duplicates share audio), else its file marked missing.
pub fn resolve(file: &str, hash: &str) -> String {
    format!(
        "coalesce(
            (SELECT id FROM tracks WHERE path = {file} AND missing = 0),
            (SELECT id FROM tracks WHERE audio_hash = {hash} AND missing = 0
             ORDER BY title IS (SELECT title FROM tracks WHERE path = {file}) DESC, id DESC LIMIT 1),
            (SELECT id FROM tracks WHERE path = {file}))"
    )
}

/// Bumped when the scanner reads files better, so rows it got wrong are
/// re-read at the next scan, or when the table changes.
const VERSION: i32 = 15;

fn upgrade(conn: &Connection, art_dir: &Path) -> rusqlite::Result<()> {
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
    if version < 8 {
        // Version 8 reads the MusicBrainz release ID (for Discord's cover
        // art): add the column and re-read every file.
        if !has("musicbrainz_album")? {
            conn.execute("ALTER TABLE tracks ADD COLUMN musicbrainz_album TEXT", [])?;
        }
        conn.execute("UPDATE tracks SET mtime = -1", [])?;
    }
    if version < 9 {
        // Version 9 reads the original release year: add the column and
        // re-read every file.
        if !has("original_year")? {
            conn.execute("ALTER TABLE tracks ADD COLUMN original_year INTEGER", [])?;
        }
        conn.execute("UPDATE tracks SET mtime = -1", [])?;
    }
    if version < 10 {
        // Version 10 reads embedded covers from their tracks instead of
        // keeping full-size copies (`<key>.<ext>`, 951 MB on a phone, #133):
        // point each at a present track and delete the copy. A copy whose
        // tracks are all missing stays, being the only one left.
        let copies = conn
            .prepare(
                "SELECT path, (SELECT t.path FROM tracks t WHERE t.art = art.key AND t.missing = 0 LIMIT 1)
                 FROM art WHERE path GLOB '*' || key || '.*'",
            )?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (copy, track) in copies {
            let Some(track) = track else { continue };
            conn.execute("UPDATE art SET path = ?1 WHERE path = ?2", [&track, &copy])?;
            let _ = std::fs::remove_file(copy);
        }
        // Copies of a track's other pictures (`Library::images`) were never
        // in the table; thumbnails are `<key>-<size>.jpg`.
        let mut referenced = conn.prepare("SELECT 1 FROM art WHERE path = ?1")?;
        for entry in std::fs::read_dir(art_dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let copy = !entry.file_name().to_string_lossy().contains('-');
            if copy && !referenced.exists([path.to_string_lossy()])? {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    if version < 11 {
        // Version 11 decodes Opus (with libopus) to hash it: check the files
        // noted as damaged again, since every Opus file was.
        conn.execute(
            "UPDATE tracks SET audio_hash = NULL, problem = NULL WHERE problem IS NOT NULL",
            [],
        )?;
    }
    if version < 12 {
        // Version 12 decodes Opus whose comment header (cover) is over 16 MiB
        // (#137): check the damaged files again.
        conn.execute(
            "UPDATE tracks SET audio_hash = NULL, problem = NULL WHERE problem IS NOT NULL",
            [],
        )?;
    }
    if version < 13 {
        // Files stored with no tags at all, from before lofty read such
        // headers (#148): read them again, since a rescan only reads changes.
        conn.execute("UPDATE tracks SET mtime = -1 WHERE codec IS NULL", [])?;
    }
    if version < 14 {
        // Version 14 notes the audio of each play, so history follows moved
        // files as playlists do, and fills in what reads used to.
        let has_hash = conn
            .prepare("SELECT 1 FROM pragma_table_info('plays') WHERE name = 'hash'")?
            .exists([])?;
        if !has_hash {
            conn.execute("ALTER TABLE plays ADD COLUMN hash TEXT", [])?;
        }
        conn.execute_batch(
            "UPDATE plays SET hash = (SELECT nullif(audio_hash, '') FROM tracks WHERE path = track_path)
             WHERE hash IS NULL;
             UPDATE playlist_items SET hash = (SELECT nullif(audio_hash, '') FROM tracks WHERE path = file)
             WHERE hash IS NULL;",
        )?;
    }
    if version < 15 {
        // Version 15 hashes lossy files by their compressed packets (#233),
        // since decoders differ between platforms: hash them again, and note
        // the old hash so playlists, plays and caches follow it.
        conn.execute_batch(
            "INSERT OR REPLACE INTO hash_moves SELECT path, audio_hash FROM tracks
             WHERE audio_hash LIKE 'pcm-b3:%' AND codec NOT IN ('FLAC', 'WAV', 'AIFF', 'WavPack', 'APE');
             UPDATE tracks SET audio_hash = NULL
             WHERE path IN (SELECT path FROM hash_moves);",
        )?;
    }
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
    fn versions_12_and_13_read_damaged_and_untagged_files_again() {
        let file = std::env::temp_dir().join(format!("motif-db12-{}.db", std::process::id()));
        let conn = open(&file).unwrap();
        conn.execute(
            "INSERT INTO tracks (path, size, mtime, audio_hash, problem)
             VALUES ('big.opus', 1, 1, '', 'ogg: packet buffer would exceed max size')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 11).unwrap();
        drop(conn);
        let conn = open(&file).unwrap();
        let row: (Option<String>, Option<String>, i64) = conn
            .query_row("SELECT audio_hash, problem, mtime FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
        assert_eq!(row, (None, None, -1), "its tags are read again too");
    }

    #[test]
    fn version_14_gives_old_plays_their_hash() {
        let file = std::env::temp_dir().join(format!("motif-db14-{}.db", std::process::id()));
        {
            // A version 13 library, from before plays had a hash.
            let conn = Connection::open(&file).unwrap();
            conn.execute_batch(
                "CREATE TABLE plays (track_path TEXT NOT NULL, played_at INTEGER NOT NULL,
                     PRIMARY KEY (track_path, played_at));
                 INSERT INTO plays VALUES ('a.flac', 100);
                 PRAGMA user_version = 13;",
            )
            .unwrap();
        }
        let conn = open(&file).unwrap();
        conn.execute(
            "INSERT INTO tracks (path, size, mtime) VALUES ('a.flac', 1, 1)",
            [],
        )
        .unwrap();
        // The scanner hashes it later.
        conn.execute(
            "UPDATE tracks SET audio_hash = 'h' WHERE path = 'a.flac'",
            [],
        )
        .unwrap();
        let hash: Option<String> = conn
            .query_row("SELECT hash FROM plays", [], |r| r.get(0))
            .unwrap();
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
        assert_eq!(hash.as_deref(), Some("h"));
    }

    #[test]
    fn version_15_moves_what_the_old_hash_keyed_to_the_packet_hash() {
        let file = std::env::temp_dir().join(format!("motif-db15-{}.db", std::process::id()));
        let conn = open(&file).unwrap();
        conn.execute_batch(
            "INSERT INTO tracks (path, size, mtime, codec, audio_hash) VALUES
                 ('a.opus', 1, 1, 'Opus', 'pcm-b3:old'),
                 ('b.wav', 1, 1, 'WAV', 'pcm-b3:wav');
             INSERT INTO playlists VALUES ('p', 'P');
             INSERT INTO playlist_items VALUES ('p', 'i', 0, 'a.opus', 'pcm-b3:old');
             INSERT INTO plays (track_path, played_at, hash) VALUES ('a.opus', 100, 'pcm-b3:old');
             INSERT INTO waveforms VALUES ('pcm-b3:old', x'01');
             INSERT INTO loudness VALUES ('pcm-b3:old', -9.0, 0.5);
             INSERT INTO lyrics VALUES ('pcm-b3:old', 's', 'p', 1);
             PRAGMA user_version = 14;",
        )
        .unwrap();
        drop(conn);
        let conn = open(&file).unwrap();
        let hashes = |sql: &str| -> Vec<Option<String>> {
            conn.prepare(sql)
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(
            hashes("SELECT audio_hash FROM tracks ORDER BY path"),
            [None, Some("pcm-b3:wav".into())],
            "lossy files are hashed again, lossless ones keep theirs"
        );
        conn.execute(
            "UPDATE tracks SET audio_hash = 'pkt-b3:new' WHERE path = 'a.opus'",
            [],
        )
        .unwrap();
        move_hash(&conn, "a.opus", "pkt-b3:new").unwrap();
        move_hash(&conn, "a.opus", "pkt-b3:new").unwrap(); // safe to repeat
        for table in ["playlist_items", "plays", "waveforms", "loudness", "lyrics"] {
            assert_eq!(
                hashes(&format!("SELECT hash FROM {table}")),
                [Some("pkt-b3:new".into())],
                "{table}"
            );
        }
        drop(conn);
        for end in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{end}", file.display()));
        }
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
