//! Saved playlists, in the `playlists` and `playlist_items` tables. Every
//! change is safe to repeat: playlists and entries carry the client's ids.

use protocol::{Playlist, PlaylistEntry};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Error, Result, TRACK_COLUMNS, library_track};

/// The library track entry `i` stands for: its file, else a present file
/// with the same audio (it moved; the same title first, since tracks of
/// silence or duplicates share audio), else its file marked missing.
const RESOLVE: &str = "coalesce(
    (SELECT id FROM tracks WHERE path = i.file AND missing = 0),
    (SELECT id FROM tracks WHERE audio_hash = i.hash AND missing = 0
     ORDER BY title IS (SELECT title FROM tracks WHERE path = i.file) DESC, id DESC LIMIT 1),
    (SELECT id FROM tracks WHERE path = i.file))";

/// Notes the audio hash of entries added before their file was hashed.
fn fill_hashes(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE playlist_items SET hash = (SELECT nullif(audio_hash, '') FROM tracks WHERE path = file)
         WHERE hash IS NULL",
        [],
    )?;
    Ok(())
}

fn exists(conn: &Connection, id: &str) -> Result<()> {
    conn.query_row("SELECT 1 FROM playlists WHERE id = ?1", [id], |_| Ok(()))
        .optional()?
        .ok_or_else(|| Error::Other(format!("no playlist {id}")))
}

fn name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Other("a playlist needs a name".into()));
    }
    Ok(name)
}

pub fn list(conn: &Connection) -> Result<Vec<Playlist>> {
    fill_hashes(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT p.id, p.name, count(tracks.id), coalesce(sum(tracks.duration_ms), 0), max(tracks.art)
         FROM playlists p
         LEFT JOIN playlist_items i ON i.playlist = p.id
         LEFT JOIN tracks ON tracks.id = {RESOLVE}
         GROUP BY p.id ORDER BY lower(p.name), p.id"
    ))?;
    let playlists = stmt
        .query_map([], |r| {
            Ok(Playlist {
                id: r.get(0)?,
                name: r.get(1)?,
                tracks: r.get(2)?,
                duration_ms: r.get::<_, i64>(3)? as u64,
                art: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(playlists)
}

pub fn entries(conn: &Connection, id: &str) -> Result<Vec<PlaylistEntry>> {
    fill_hashes(conn)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRACK_COLUMNS}, i.item FROM playlist_items i
         JOIN tracks ON tracks.id = {RESOLVE}
         WHERE i.playlist = ?1 ORDER BY i.pos"
    ))?;
    let entries = stmt
        .query_map([id], |r| {
            Ok(PlaylistEntry {
                track: library_track(r)?,
                item: r.get(23)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(entries)
}

/// The playlist's name and tracks, for playing it.
pub fn named_entries(conn: &Connection, id: &str) -> Result<(String, Vec<PlaylistEntry>)> {
    let name = conn
        .query_row("SELECT name FROM playlists WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::Other(format!("no playlist {id}")))?;
    Ok((name, entries(conn, id)?))
}

pub fn create(conn: &Connection, id: &str, name_: &str, paths: &[String]) -> Result<()> {
    let name = name(name_)?;
    if id.is_empty() {
        return Err(Error::Other("a playlist needs an id".into()));
    }
    let tx = conn.unchecked_transaction()?;
    // Made already: a repeat, so its tracks are in too.
    if tx.execute(
        "INSERT OR IGNORE INTO playlists (id, name) VALUES (?1, ?2)",
        [id, name],
    )? == 1
    {
        append(&tx, id, id, paths)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn add(conn: &Connection, id: &str, op: &str, paths: &[String]) -> Result<()> {
    if op.is_empty() {
        return Err(Error::Other("an addition needs an id".into()));
    }
    let tx = conn.unchecked_transaction()?;
    exists(&tx, id)?;
    append(&tx, id, op, paths)?;
    tx.commit()?;
    Ok(())
}

/// Adds entries `<op>/<index>` at the end; ones already there stay put.
fn append(conn: &Connection, id: &str, op: &str, paths: &[String]) -> Result<()> {
    let end: i64 = conn.query_row(
        "SELECT coalesce(max(pos) + 1, 0) FROM playlist_items WHERE playlist = ?1",
        [id],
        |r| r.get(0),
    )?;
    let mut insert = conn.prepare(
        "INSERT OR IGNORE INTO playlist_items (playlist, item, pos, file, hash)
         VALUES (?1, ?2, ?3, ?4, (SELECT nullif(audio_hash, '') FROM tracks WHERE path = ?4))",
    )?;
    for (i, path) in paths.iter().enumerate() {
        insert.execute(params![id, format!("{op}/{i}"), end + i as i64, path])?;
    }
    Ok(())
}

pub fn rename(conn: &Connection, id: &str, name_: &str) -> Result<()> {
    let name = name(name_)?;
    if conn.execute("UPDATE playlists SET name = ?2 WHERE id = ?1", [id, name])? == 0 {
        return Err(Error::Other(format!("no playlist {id}")));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM playlist_items WHERE playlist = ?1", [id])?;
    tx.execute("DELETE FROM playlists WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

pub fn remove(conn: &Connection, id: &str, items: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let mut delete = tx.prepare("DELETE FROM playlist_items WHERE playlist = ?1 AND item = ?2")?;
    for item in items {
        delete.execute([id, item])?;
    }
    drop(delete);
    tx.commit()?;
    Ok(())
}

/// Moves `item` before `before` (to the end if `None`). An entry already
/// removed stays removed; one to go before that is gone means the end.
pub fn move_item(conn: &Connection, id: &str, item: &str, before: Option<&str>) -> Result<()> {
    if before == Some(item) {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let mut order: Vec<String> = tx
        .prepare("SELECT item FROM playlist_items WHERE playlist = ?1 ORDER BY pos")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let Some(from) = order.iter().position(|i| i == item) else {
        return Ok(());
    };
    let moved = order.remove(from);
    let to = before
        .and_then(|b| order.iter().position(|i| i == b))
        .unwrap_or(order.len());
    order.insert(to, moved);
    let mut set =
        tx.prepare("UPDATE playlist_items SET pos = ?3 WHERE playlist = ?1 AND item = ?2")?;
    for (pos, item) in order.iter().enumerate() {
        set.execute(params![id, item, pos as i64])?;
    }
    drop(set);
    tx.commit()?;
    Ok(())
}
