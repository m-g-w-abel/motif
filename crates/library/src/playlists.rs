//! Saved playlists, in the `playlists` and `playlist_items` tables. Every
//! change is safe to repeat: playlists and entries carry the client's ids.
//! Changes are logged as sync ops (sync.rs), which make them.

use protocol::{Playlist, PlaylistEntry};
use rusqlite::{Connection, OptionalExtension};

use crate::{
    Error, Result, TRACK_COLUMNS, db, library_track,
    sync::{self, Op},
};

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
    let mut stmt = conn.prepare(&format!(
        "SELECT p.id, p.name, count(tracks.id), coalesce(sum(tracks.duration_ms), 0), max(tracks.art)
         FROM playlists p
         LEFT JOIN playlist_items i ON i.playlist = p.id
         LEFT JOIN tracks ON tracks.id = {}
         GROUP BY p.id ORDER BY lower(p.name), p.id",
        db::resolve("i.file", "i.hash")
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
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRACK_COLUMNS}, i.item FROM playlist_items i
         JOIN tracks ON tracks.id = {}
         WHERE i.playlist = ?1 ORDER BY i.pos, i.item",
        db::resolve("i.file", "i.hash")
    ))?;
    let entries = stmt
        .query_map([id], |r| {
            Ok(PlaylistEntry {
                track: library_track(r)?,
                item: r.get(25)?,
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

pub fn create(
    conn: &Connection,
    device: &str,
    id: &str,
    name_: &str,
    paths: &[String],
) -> Result<()> {
    let name = name(name_)?;
    if id.is_empty() {
        return Err(Error::Other("a playlist needs an id".into()));
    }
    let tx = conn.unchecked_transaction()?;
    // Made already (or deleted since, which stays): a repeat, so its tracks are in too.
    if exists(&tx, id).is_err() && !sync::tombstoned(&tx, &format!("deleted/{id}"))? {
        let op = Op::Name {
            playlist: id.into(),
            name: name.into(),
        };
        sync::record(&tx, device, op, None)?;
        append(&tx, device, id, id, paths)?;
    }
    tx.commit()?;
    Ok(())
}

pub fn add(conn: &Connection, device: &str, id: &str, op: &str, paths: &[String]) -> Result<()> {
    if op.is_empty() {
        return Err(Error::Other("an addition needs an id".into()));
    }
    let tx = conn.unchecked_transaction()?;
    exists(&tx, id)?;
    append(&tx, device, id, op, paths)?;
    tx.commit()?;
    Ok(())
}

/// Adds entries `<op>/<index>` at the end; ones already there (or removed
/// since) stay as they are, and files not in the library are left out.
fn append(conn: &Connection, device: &str, id: &str, op: &str, paths: &[String]) -> Result<()> {
    let end: f64 = conn.query_row(
        "SELECT coalesce(max(pos) + 1, 0) FROM playlist_items WHERE playlist = ?1",
        [id],
        |r| r.get(0),
    )?;
    for (i, path) in paths.iter().enumerate() {
        let item = format!("{op}/{i}");
        let there = conn
            .query_row(
                "SELECT 1 FROM playlist_items WHERE playlist = ?1 AND item = ?2",
                [id, &item],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if there || sync::tombstoned(conn, &format!("removed/{id}/{item}"))? {
            continue;
        }
        let Some(track) = sync::track_at(conn, path)? else {
            continue;
        };
        let op = Op::Add {
            playlist: id.into(),
            item,
            track,
            pos: end + i as f64,
        };
        sync::record(conn, device, op, Some(path))?;
    }
    Ok(())
}

pub fn rename(conn: &Connection, device: &str, id: &str, name_: &str) -> Result<()> {
    let name = name(name_)?;
    let old: Option<String> = conn
        .query_row("SELECT name FROM playlists WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    match old {
        None => Err(Error::Other(format!("no playlist {id}"))),
        Some(old) if old == name => Ok(()),
        Some(_) => {
            let op = Op::Name {
                playlist: id.into(),
                name: name.into(),
            };
            sync::record(conn, device, op, None)?;
            Ok(())
        }
    }
}

pub fn delete(conn: &Connection, device: &str, id: &str) -> Result<()> {
    if exists(conn, id).is_ok() {
        let op = Op::Delete {
            playlist: id.into(),
        };
        sync::record(conn, device, op, None)?;
    }
    Ok(())
}

pub fn remove(conn: &Connection, device: &str, id: &str, items: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for item in items {
        let there = tx
            .query_row(
                "SELECT 1 FROM playlist_items WHERE playlist = ?1 AND item = ?2",
                [id, item],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if there {
            let op = Op::Remove {
                playlist: id.into(),
                item: item.clone(),
            };
            sync::record(&tx, device, op, None)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Moves `item` before `before` (to the end if `None`), halfway between its
/// new neighbours so only it changes. An entry already removed stays
/// removed; one to go before that is gone means the end.
// shortcut: f64 halves run out after ~50 moves into the same gap, then ties
// fall back to item order (nothing is lost); use string keys if that shows.
pub fn move_item(
    conn: &Connection,
    device: &str,
    id: &str,
    item: &str,
    before: Option<&str>,
) -> Result<()> {
    if before == Some(item) {
        return Ok(());
    }
    let mut order: Vec<(String, f64)> = conn
        .prepare("SELECT item, pos FROM playlist_items WHERE playlist = ?1 ORDER BY pos, item")?
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let Some(from) = order.iter().position(|(i, _)| i == item) else {
        return Ok(());
    };
    order.remove(from);
    let to = before
        .and_then(|b| order.iter().position(|(i, _)| i == b))
        .unwrap_or(order.len());
    if to == from {
        return Ok(());
    }
    let pos = match (
        to.checked_sub(1).map(|i| order[i].1),
        order.get(to).map(|o| o.1),
    ) {
        (Some(prev), Some(next)) => (prev + next) / 2.0,
        (Some(prev), None) => prev + 1.0,
        (None, Some(next)) => next - 1.0,
        (None, None) => 0.0,
    };
    let op = Op::Move {
        playlist: id.into(),
        item: item.into(),
        pos,
    };
    sync::record(conn, device, op, None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use protocol::{LibraryQuery, QueryResult};

    use crate::Library;

    #[test]
    fn an_entry_added_before_its_file_was_hashed_follows_a_move() {
        let dir = std::env::temp_dir().join(format!(
            "motif-playlists-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        library
            .db()
            .execute(
                "INSERT INTO tracks (path, size, mtime, title) VALUES ('/m/a.flac', 1, 1, 'A')",
                [],
            )
            .unwrap();
        let create = LibraryQuery::CreatePlaylist {
            id: "p".into(),
            name: "P".into(),
            paths: vec!["/m/a.flac".into()],
        };
        library.query(create).unwrap();
        // Hashed by the scanner, then moved.
        library
            .db()
            .execute_batch(
                "UPDATE tracks SET audio_hash = 'h1' WHERE path = '/m/a.flac';
                 UPDATE tracks SET missing = 1 WHERE path = '/m/a.flac';
                 INSERT INTO tracks (path, size, mtime, title, audio_hash)
                 VALUES ('/m/moved.flac', 1, 1, 'A', 'h1');",
            )
            .unwrap();
        let Ok(QueryResult::PlaylistTracks { entries }) =
            library.query(LibraryQuery::PlaylistTracks { id: "p".into() })
        else {
            panic!()
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].track.track.id, "/m/moved.flac");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
