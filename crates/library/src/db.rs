use std::path::Path;

use rusqlite::{Connection, params};

/// Every list uses this order: album artist, album, disc, track.
pub const ORDER: &str = "ORDER BY lower(coalesce(album_artist, artist, '')), lower(coalesce(album, '')), \
     coalesce(disc_no, 1), coalesce(track_no, 0), path";

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
         CREATE TABLE IF NOT EXISTS art (key TEXT PRIMARY KEY, path TEXT NOT NULL);
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
             art TEXT
         );
         CREATE INDEX IF NOT EXISTS tracks_mb ON tracks (musicbrainz_recording);
         CREATE INDEX IF NOT EXISTS tracks_isrc ON tracks (isrc);
         CREATE INDEX IF NOT EXISTS tracks_hash ON tracks (audio_hash);
         CREATE INDEX IF NOT EXISTS tracks_title ON tracks (lower(title));
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
    Ok(conn)
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

pub fn roots(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT path FROM roots ORDER BY path")?;
    stmt.query_map([], |row| row.get(0))?.collect()
}

pub fn track_count(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("SELECT count(*) FROM tracks", params![], |row| row.get(0))
}
