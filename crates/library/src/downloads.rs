//! The Downloaded group (#245): what this device keeps copies of, in
//! `download_members` (playlists, albums, artists, single tracks), and the
//! copies Motif downloaded, in `downloads`. The group is this device's own
//! and isn't synced. Members are matched against the library each time, so
//! a playlist's edits and an artist's later albums follow; the server
//! fetches the linked engines' tracks they take in ([`Library::download_plan`])
//! and deletes copies no member needs any more. Only copies listed in
//! `downloads` are ever deleted, never the user's own files.

use std::path::{Path, PathBuf};

use protocol::{DownloadStatus, Downloadable, QueryResult};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Error, Library, Result, db};

pub(crate) const SCHEMA: &str = "
    -- `key`: a playlist's id, an album key, an artist's name, a track's
    -- path; `hash`: a track's audio, which finds it again once it moves.
    CREATE TABLE IF NOT EXISTS download_members (
        kind TEXT NOT NULL, key TEXT NOT NULL, hash TEXT, PRIMARY KEY (kind, key)
    );
    -- A copy Motif downloaded: its file here, the catalog row it came from
    -- (`remote:<engine>:<id>`) and that track's audio hash, which the copy
    -- takes (an Opus copy hashes differently).
    CREATE TABLE IF NOT EXISTS downloads (path TEXT PRIMARY KEY, source TEXT NOT NULL, hash TEXT);
";

/// A linked engine's track to download.
#[derive(Debug, Clone, PartialEq)]
pub struct Wanted {
    /// Its catalog row's path, `remote:<engine>:<id>`.
    pub source: String,
    pub hash: Option<String>,
    /// Where to save it in the download folder, without an extension:
    /// `Artist/Album/01 Title`.
    pub name: PathBuf,
    /// Its format and bitrate (kb/s), which say whether it comes encoded
    /// ([`crate::transcode::plan`]).
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
}

impl Wanted {
    /// The extension of a copy of the original file.
    pub fn extension(&self) -> String {
        let codec = self.codec.as_deref().unwrap_or_default();
        let ext = match codec {
            "Vorbis" => "ogg",
            "WavPack" => "wv",
            other => &other.to_ascii_lowercase(),
        };
        match crate::tags::AUDIO_EXTENSIONS.contains(&ext) {
            true => ext.into(),
            // shortcut: an MP4 container shows as its extension; an odd one is guessed as M4A.
            false => "m4a".into(),
        }
    }
}

/// What downloading the Downloaded group takes now.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub fetch: Vec<Wanted>,
    /// Copies no member needs any more.
    pub remove: Vec<PathBuf>,
}

/// A track a member takes in.
struct Matched {
    path: String,
    device: String,
    hash: Option<String>,
    missing: bool,
    name: PathBuf,
    codec: Option<String>,
    bitrate: Option<u32>,
}

const MATCHED: &str = "tracks.path, tracks.device, nullif(tracks.audio_hash, ''), tracks.missing,
    coalesce(album_artist, artist), album, disc_no, track_no, title, codec, bitrate";

fn matched(r: &Row) -> rusqlite::Result<Matched> {
    let part = |i: usize| -> rusqlite::Result<String> {
        let text: Option<String> = r.get(i)?;
        Ok(safe(text.as_deref().unwrap_or("Unknown")))
    };
    let (disc, number): (Option<u32>, Option<u32>) = (r.get(6)?, r.get(7)?);
    let title = match (disc, number) {
        (Some(d), Some(n)) if d > 1 => format!("{d}-{n:02} {}", part(8)?),
        (_, Some(n)) => format!("{n:02} {}", part(8)?),
        _ => part(8)?,
    };
    Ok(Matched {
        path: r.get(0)?,
        device: r.get(1)?,
        hash: r.get(2)?,
        missing: r.get(3)?,
        name: [part(4)?, part(5)?, title].iter().collect(),
        codec: r.get(9)?,
        bitrate: r.get(10)?,
    })
}

/// `name` as one file or folder name anywhere: no separators, nothing
/// Windows or Android's storage refuse, never `.` or `..`.
fn safe(name: &str) -> String {
    let bad = |c: char| c.is_control() || "/\\:*?\"<>|".contains(c);
    let name: String = name.chars().map(|c| if bad(c) { '_' } else { c }).collect();
    let name = name.trim().trim_end_matches('.').trim();
    let name: String = name.chars().take(100).collect();
    match name.as_str() {
        "" => "_".into(),
        _ => name,
    }
}

fn kind(member: &Downloadable) -> (&'static str, &str) {
    match member {
        Downloadable::Playlist { id } => ("playlist", id),
        Downloadable::Album { album } => ("album", album),
        Downloadable::Artist { name } => ("artist", name),
        Downloadable::Track { path } => ("track", path),
    }
}

fn members(conn: &Connection) -> Result<Vec<(Downloadable, Option<String>)>> {
    let mut stmt =
        conn.prepare("SELECT kind, key, hash FROM download_members ORDER BY kind, key")?;
    let rows = stmt.query_map([], |r| {
        let (kind, key): (String, String) = (r.get(0)?, r.get(1)?);
        let member = match kind.as_str() {
            "playlist" => Downloadable::Playlist { id: key },
            "album" => Downloadable::Album { album: key },
            "artist" => Downloadable::Artist { name: key },
            _ => Downloadable::Track { path: key },
        };
        Ok((member, r.get(2)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The tracks `member` takes in, on any engine.
fn tracks_of(conn: &Connection, member: &Downloadable, hash: Option<&str>) -> Result<Vec<Matched>> {
    let (sql, key) = match member {
        Downloadable::Playlist { id } => (
            format!(
                "SELECT {MATCHED} FROM playlist_items i JOIN tracks ON tracks.id = {}
                 WHERE i.playlist = ?1",
                db::resolve("i.file", "i.hash")
            ),
            id,
        ),
        Downloadable::Album { album } => (
            format!("SELECT {MATCHED} FROM tracks WHERE {} = ?1", db::ALBUM_KEY),
            album,
        ),
        Downloadable::Artist { name } => (
            format!("SELECT {MATCHED} FROM tracks WHERE coalesce(album_artist, artist) = ?1"),
            name,
        ),
        Downloadable::Track { path } => (
            format!("SELECT {MATCHED} FROM tracks WHERE path = ?1 OR audio_hash = ?2"),
            path,
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let params: &[&dyn rusqlite::ToSql] = match member {
        Downloadable::Track { .. } => &[key, &hash],
        _ => &[key],
    };
    Ok(stmt
        .query_map(params, matched)?
        .collect::<rusqlite::Result<_>>()?)
}

impl Library {
    /// What the Downloaded group holds.
    pub(crate) fn downloaded(&self) -> Result<QueryResult> {
        let members = members(&self.db())?.into_iter().map(|(m, _)| m).collect();
        Ok(QueryResult::Downloaded { members })
    }

    /// Adds `member` to the Downloaded group, or takes it out. Safe to repeat.
    pub(crate) fn set_downloaded(&self, member: &Downloadable, downloaded: bool) -> Result<()> {
        let conn = self.db();
        let (kind, key) = kind(member);
        if !downloaded {
            conn.execute(
                "DELETE FROM download_members WHERE kind = ?1 AND key = ?2",
                [kind, key],
            )?;
        } else {
            let hash: Option<String> = match member {
                Downloadable::Track { path } => conn
                    .query_row(
                        "SELECT nullif(audio_hash, '') FROM tracks WHERE path = ?1",
                        [path],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| Error::Other(format!("no track {path}")))?,
                _ => None,
            };
            conn.execute(
                "INSERT OR IGNORE INTO download_members (kind, key, hash) VALUES (?1, ?2, ?3)",
                params![kind, key, hash],
            )?;
        }
        drop(conn);
        // The server's downloader follows the library's changes.
        self.shared.update(|s| s.revision += 1);
        Ok(())
    }

    /// Where downloads are saved: the setting, else Music/Motif.
    pub fn download_folder(&self) -> PathBuf {
        let set: Option<String> = self
            .db()
            .query_row(
                "SELECT value FROM settings WHERE key = 'download_folder'",
                [],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        set.map(PathBuf::from).unwrap_or_else(default_folder)
    }

    /// Saves downloads to `path` from now on; those made already stay where
    /// they are.
    pub(crate) fn set_download_folder(&self, path: &str) -> Result<()> {
        if !Path::new(path).is_absolute() {
            return Err(Error::Other(format!("{path} is not a full path")));
        }
        self.db().execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('download_folder', ?1)",
            [path],
        )?;
        let folder = path.to_string();
        self.shared.update(|s| s.downloads.folder = folder);
        Ok(())
    }

    /// The linked engines' tracks the Downloaded group takes in that aren't
    /// here, and the copies Motif downloaded that no member needs any more.
    pub fn download_plan(&self) -> Result<Plan> {
        let conn = self.db();
        let mut all = Vec::new();
        for (member, hash) in members(&conn)? {
            all.extend(tracks_of(&conn, &member, hash.as_deref())?);
        }
        let here = |hash: &Option<String>| -> Result<bool> {
            let Some(hash) = hash else { return Ok(false) };
            Ok(conn
                .prepare_cached(
                    "SELECT 1 FROM tracks WHERE audio_hash = ?1 AND device = '' AND missing = 0
                     UNION SELECT 1 FROM downloads WHERE hash = ?1",
                )?
                .exists([hash])?)
        };
        let downloaded = |source: &str| -> Result<bool> {
            Ok(conn
                .prepare_cached("SELECT 1 FROM downloads WHERE source = ?1")?
                .exists([source])?)
        };
        let mut plan = Plan::default();
        let mut taken = std::collections::HashSet::new();
        for track in &all {
            let key = track.hash.clone().unwrap_or_else(|| track.path.clone());
            if track.device.is_empty()
                || track.missing
                || !taken.insert(key)
                || here(&track.hash)?
                || downloaded(&track.path)?
            {
                continue;
            }
            plan.fetch.push(Wanted {
                source: track.path.clone(),
                hash: track.hash.clone(),
                name: track.name.clone(),
                codec: track.codec.clone(),
                bitrate: track.bitrate,
            });
        }
        let needed = |path: &str, source: &str, hash: &Option<String>| {
            all.iter()
                .any(|t| t.path == path || t.path == source || (hash.is_some() && t.hash == *hash))
        };
        let mut stmt = conn.prepare("SELECT path, source, hash FROM downloads")?;
        let copies = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })?;
        for copy in copies {
            let (path, source, hash) = copy?;
            if !needed(&path, &source, &hash) {
                plan.remove.push(path.into());
            }
        }
        Ok(plan)
    }

    /// Vorbis comments for an encoded copy of the catalog row `source`:
    /// what its owner's file says.
    pub fn download_tags(&self, source: &str) -> Vec<(String, String)> {
        const NAMES: [&str; 12] = [
            "TITLE",
            "ARTIST",
            "ALBUM",
            "ALBUMARTIST",
            "TRACKNUMBER",
            "DISCNUMBER",
            "DATE",
            "ORIGINALDATE",
            "GENRE",
            "MUSICBRAINZ_TRACKID",
            "MUSICBRAINZ_ALBUMID",
            "ISRC",
        ];
        let row = self.db().query_row(
            "SELECT title, artist, album, album_artist, track_no, disc_no, year, original_year,
                    genre, musicbrainz_recording, musicbrainz_album, isrc
             FROM tracks WHERE path = ?1",
            [source],
            |r| {
                (0..NAMES.len())
                    .map(|i| r.get::<_, Option<rusqlite::types::Value>>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            },
        );
        let values = row.unwrap_or_default();
        NAMES
            .iter()
            .zip(values)
            .filter_map(|(name, value)| {
                let value = match value? {
                    rusqlite::types::Value::Text(text) => text,
                    rusqlite::types::Value::Integer(n) => n.to_string(),
                    _ => return None,
                };
                Some((name.to_string(), value))
            })
            .collect()
    }

    /// Notes the copy of `wanted` saved at `path`, and scans it into the
    /// library (adding the download folder, if no library folder holds it).
    /// Its cover goes next to it, as an encoded copy has none of its own.
    pub fn add_download(&self, path: &Path, wanted: &Wanted) -> Result<()> {
        let folder = self.download_folder();
        let art: Option<String> = self
            .db()
            .query_row(
                "SELECT art FROM tracks WHERE path = ?1",
                [&wanted.source],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let (Some(key), Some(dir)) = (art, path.parent())
            && let Ok(image) = self.art_source(&key)
        {
            self.save_cover(&image, dir);
        }
        let conn = self.db();
        let text = path
            .to_str()
            .ok_or_else(|| Error::Other("file name is not UTF-8".into()))?;
        conn.execute(
            "INSERT OR REPLACE INTO downloads (path, source, hash) VALUES (?1, ?2, ?3)",
            params![text, wanted.source, wanted.hash],
        )?;
        let roots = db::roots(&conn)?;
        if !roots.iter().any(|root| path.starts_with(root)) {
            std::fs::create_dir_all(&folder)?;
            let folder = std::fs::canonicalize(&folder)?;
            let folder = folder
                .to_str()
                .ok_or_else(|| Error::Other("folder name is not UTF-8".into()))?;
            conn.execute("INSERT OR IGNORE INTO roots (path) VALUES (?1)", [folder])?;
            self.roots_changed(&conn)?;
        } else {
            let _ = self.scanner.send(crate::scan::Request::Changed);
        }
        Ok(())
    }

    /// Deletes the copy Motif downloaded to `path`, and its track. Does
    /// nothing to a file it didn't download.
    pub fn remove_download(&self, path: &Path) -> Result<()> {
        let top = self.download_folder();
        let conn = self.db();
        let Some(text) = path.to_str() else {
            return Ok(());
        };
        let ours = conn
            .prepare("SELECT 1 FROM downloads WHERE path = ?1")?
            .exists([text])?;
        if !ours {
            return Ok(());
        }
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
        // Folders it leaves empty go too (but for the cover added with it),
        // up to the download folder.
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| d.starts_with(&top) && *d != top) {
            let left: Vec<_> = std::fs::read_dir(d)?
                .flatten()
                .map(|f| f.file_name())
                .collect();
            if let [only] = &left[..]
                && (only == "cover.jpg" || only == "cover.png")
            {
                let _ = std::fs::remove_file(d.join(only));
            }
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
        conn.execute("DELETE FROM downloads WHERE path = ?1", [text])?;
        conn.execute("DELETE FROM tracks WHERE path = ?1 AND device = ''", [text])?;
        drop(conn);
        self.shared.update(|s| {
            s.tracks = s.tracks.saturating_sub(1);
            s.revision += 1;
        });
        Ok(())
    }

    /// Says how downloading is going.
    pub fn set_download_status(&self, waiting: u32, error: Option<String>) {
        let folder = self.download_folder().to_string_lossy().into_owned();
        let now = DownloadStatus {
            folder,
            waiting,
            error,
        };
        self.shared
            .status
            .send_if_modified(|s| (s.downloads != now).then(|| s.downloads = now).is_some());
    }
}

impl Library {
    /// Copies `image` to `dir` as its folder's cover, unless it has one.
    fn save_cover(&self, image: &Path, dir: &Path) {
        let Ok(bytes) = std::fs::read(image) else {
            return;
        };
        let ext = match image::guess_format(&bytes) {
            Ok(image::ImageFormat::Png) => "png",
            Ok(image::ImageFormat::Jpeg) => "jpg",
            _ => return,
        };
        let has = |name: &str| {
            ["jpg", "png"]
                .iter()
                .any(|e| dir.join(format!("{name}.{e}")).exists())
        };
        if !has("cover") && !has("folder") {
            let _ = std::fs::write(dir.join(format!("cover.{ext}")), bytes);
        }
    }
}

/// Once the linked engine's row `source` has its audio `hash`, the copies
/// downloaded from it before it had one take it too, in place of their own,
/// so they stand in for it.
pub(crate) fn source_hashed(conn: &Connection, source: &str, hash: &str) -> rusqlite::Result<()> {
    let copies: Vec<String> = conn
        .prepare_cached("SELECT path FROM downloads WHERE source = ?1 AND coalesce(hash, '') = ''")?
        .query_map([source], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    for copy in copies {
        conn.execute(
            "UPDATE downloads SET hash = ?1 WHERE path = ?2",
            [hash, &copy],
        )?;
        // What its own hash keyed (plays, caches) follows it.
        conn.execute(
            "INSERT OR REPLACE INTO hash_moves (path, old) SELECT path, audio_hash FROM tracks
             WHERE path = ?1 AND device = '' AND coalesce(audio_hash, '') != ''",
            [&copy],
        )?;
        conn.execute(
            "UPDATE tracks SET audio_hash = ?1 WHERE path = ?2 AND device = ''",
            [hash, &copy],
        )?;
        db::move_hash(conn, &copy, hash)?;
    }
    Ok(())
}

/// The audio hash a downloaded copy at `path` takes: its source's.
pub(crate) fn source_hash(conn: &Connection, path: &str) -> Option<String> {
    conn.query_row(
        "SELECT hash FROM downloads WHERE path = ?1 AND hash != ''",
        [path],
        |r| r.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

fn default_folder() -> PathBuf {
    if cfg!(target_os = "android") {
        return PathBuf::from("/storage/emulated/0/Music/Motif");
    }
    let music = dirs::audio_dir().or_else(|| Some(dirs::home_dir()?.join("Music")));
    music.unwrap_or_default().join("Motif")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(name: &str) -> (Library, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("motif-downloads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir).unwrap();
        let folder = dir.join("Motif");
        library
            .set_download_folder(folder.to_str().unwrap())
            .unwrap();
        (library, dir)
    }

    /// A track of linked engine `e`, as its catalog puts it here.
    fn remote(library: &Library, id: u32, title: &str, album: &str, hash: &str) -> String {
        let path = format!("remote:e:{id}");
        library
            .db()
            .execute(
                "INSERT INTO tracks (path, size, mtime, title, artist, album, track_no, codec, audio_hash, device)
                 VALUES (?1, 0, 0, ?2, 'Band', ?3, ?4, 'FLAC', ?5, 'e')",
                params![path, title, album, id, hash],
            )
            .unwrap();
        path
    }

    fn album(name: &str) -> Downloadable {
        Downloadable::Album {
            album: format!("Band\u{1f}{name}"),
        }
    }

    fn fetched(library: &Library) -> Vec<String> {
        let plan = library.download_plan().unwrap();
        let mut sources: Vec<_> = plan.fetch.into_iter().map(|w| w.source).collect();
        sources.sort();
        sources
    }

    /// Downloads everything planned, as the server does (an empty file each).
    fn download(library: &Library) -> Vec<PathBuf> {
        let folder = library.download_folder();
        let mut saved = Vec::new();
        for wanted in library.download_plan().unwrap().fetch {
            let path = folder.join(&wanted.name).with_extension("flac");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"").unwrap();
            library.add_download(&path, &wanted).unwrap();
            saved.push(path);
        }
        saved
    }

    fn removed(library: &Library) -> Vec<PathBuf> {
        let mut removed = library.download_plan().unwrap().remove;
        for path in &removed {
            library.remove_download(path).unwrap();
        }
        removed.sort();
        removed
    }

    #[test]
    fn adding_an_album_downloads_its_tracks_and_removing_it_deletes_them() {
        let (library, dir) = library("album");
        let a1 = remote(&library, 1, "One", "Alpha", "h1");
        let a2 = remote(&library, 2, "Two", "Alpha", "h2");
        remote(&library, 3, "Three", "Beta", "h3");
        library.set_downloaded(&album("Alpha"), true).unwrap();
        // Again: safe to repeat.
        library.set_downloaded(&album("Alpha"), true).unwrap();
        assert_eq!(fetched(&library), [a1, a2]);
        let plan = library.download_plan().unwrap();
        assert_eq!(plan.fetch[0].name, Path::new("Band/Alpha/01 One"));
        let saved = download(&library);
        assert!(fetched(&library).is_empty(), "downloaded twice");
        assert!(removed(&library).is_empty());

        library.set_downloaded(&album("Alpha"), false).unwrap();
        assert_eq!(removed(&library), saved);
        assert!(saved.iter().all(|p| !p.exists()));
        // Its folders went with it.
        assert!(!library.download_folder().join("Band").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_playlist_follows_its_edits() {
        let (library, dir) = library("playlist");
        let a1 = remote(&library, 1, "One", "Alpha", "h1");
        let a2 = remote(&library, 2, "Two", "Alpha", "h2");
        let device = "me";
        crate::playlists::create(&library.db(), device, "p", "Mix", std::slice::from_ref(&a1))
            .unwrap();
        let member = Downloadable::Playlist { id: "p".into() };
        library.set_downloaded(&member, true).unwrap();
        assert_eq!(fetched(&library), std::slice::from_ref(&a1));
        download(&library);
        crate::playlists::add(&library.db(), device, "p", "op", std::slice::from_ref(&a2)).unwrap();
        assert_eq!(fetched(&library), [a2]);
        download(&library);
        let first = crate::playlists::entries(&library.db(), "p").unwrap()[0]
            .item
            .clone();
        crate::playlists::remove(&library.db(), device, "p", &[first]).unwrap();
        let gone = removed(&library);
        assert_eq!(gone.len(), 1);
        assert!(gone[0].ends_with("Band/Alpha/01 One.flac"), "{gone:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_track_another_member_needs_survives_removing_one() {
        let (library, dir) = library("shared");
        let a1 = remote(&library, 1, "One", "Alpha", "h1");
        remote(&library, 2, "Two", "Alpha", "h2");
        library.set_downloaded(&album("Alpha"), true).unwrap();
        let song = Downloadable::Track { path: a1 };
        library.set_downloaded(&song, true).unwrap();
        let saved = download(&library);
        assert_eq!(saved.len(), 2, "the shared track downloaded twice");
        library.set_downloaded(&album("Alpha"), false).unwrap();
        let gone = removed(&library);
        assert_eq!(gone.len(), 1);
        assert!(gone[0].ends_with("02 Two.flac"), "{gone:?}");
        assert!(saved[0].exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_copy_downloaded_before_its_source_was_hashed_takes_its_hash_once_it_comes() {
        use crate::catalog::{CatalogTrack, Change, Page};
        let (library, dir) = library("late-hash");
        let page = |from, audio_hash: Option<&str>| Page {
            epoch: "x".into(),
            from,
            changes: vec![Change {
                version: from + 1,
                id: 1,
                track: Some(CatalogTrack {
                    title: Some("One".into()),
                    artist: Some("Band".into()),
                    album: Some("Alpha".into()),
                    track_no: Some(1),
                    codec: Some("FLAC".into()),
                    audio_hash: audio_hash.map(Into::into),
                    ..Default::default()
                }),
            }],
        };
        library.merge_catalog("e", page(0, None)).unwrap();
        library.set_downloaded(&album("Alpha"), true).unwrap();
        let wanted = library.download_plan().unwrap().fetch.remove(0);
        assert_eq!(wanted.hash, None);
        let copy = library.download_folder().join("01 One.opus");
        library.add_download(&copy, &wanted).unwrap();
        let copy = copy.to_str().unwrap();
        // Hashed by the scanner as its own audio, its source's being unknown.
        library
            .db()
            .execute(
                "INSERT INTO tracks (path, size, mtime, title, artist, album, track_no, audio_hash)
                 VALUES (?1, 0, 0, 'One', 'Band', 'Alpha', 1, 'pkt-b3:own')",
                [copy],
            )
            .unwrap();

        // The owner hashes it.
        library.merge_catalog("e", page(1, Some("h1"))).unwrap();
        let conn = library.db();
        let copy_hash: String = conn
            .query_row(
                "SELECT audio_hash FROM tracks WHERE path = ?1",
                [copy],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(copy_hash, "h1");
        let shadowed: bool = conn
            .query_row(
                "SELECT shadowed FROM tracks WHERE path = ?1",
                [&wanted.source],
                |r| r.get(0),
            )
            .unwrap();
        assert!(shadowed, "the linked engine's row still shows");
        drop(conn);
        assert!(fetched(&library).is_empty());
        assert!(removed(&library).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_artist_takes_in_later_albums_and_nothing_already_here() {
        let (library, dir) = library("artist");
        let a1 = remote(&library, 1, "One", "Alpha", "h1");
        // The user's own copy of track 2.
        remote(&library, 2, "Two", "Alpha", "h2");
        library
            .db()
            .execute(
                "INSERT INTO tracks (path, size, mtime, title, artist, album, audio_hash)
                 VALUES ('/mine/two.flac', 0, 0, 'Two', 'Band', 'Alpha', 'h2')",
                [],
            )
            .unwrap();
        let artist = Downloadable::Artist {
            name: "Band".into(),
        };
        library.set_downloaded(&artist, true).unwrap();
        assert_eq!(fetched(&library), std::slice::from_ref(&a1));
        let later = remote(&library, 3, "Three", "Gamma", "h3");
        assert_eq!(fetched(&library), [a1, later]);
        // Never the user's own file.
        library.set_downloaded(&artist, false).unwrap();
        assert!(library.download_plan().unwrap().remove.is_empty());
        let QueryResult::Downloaded { members } = library.downloaded().unwrap() else {
            panic!()
        };
        assert!(members.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn names_are_safe_anywhere() {
        assert_eq!(safe("AC/DC"), "AC_DC");
        assert_eq!(safe(".."), "_");
        assert_eq!(safe(" What? "), "What_");
        assert_eq!(safe("Etc..."), "Etc");
    }
}
