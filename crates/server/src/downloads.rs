//! Downloads the Downloaded group (#245): fetches the linked engines'
//! tracks its members take in ([`Library::download_plan`]) whole into the
//! [`files::Store`] at the download quality, moves each into the download
//! folder, where the library scans it in, and deletes the copies no member
//! needs any more. Follows the library as it changes; what failed is tried
//! again later. Safe to stop at any point: it carries on where it was.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use library::{Library, downloads::Wanted, transcode};
use protocol::{Fetching, Quality, TrackKeys, TrackRef};

use crate::{Pairing, files};

/// How long a failed download waits to be tried again.
const RETRY: Duration = Duration::from_secs(60);

pub(crate) async fn run(library: Library, pairing: Pairing) {
    let mut status = library.subscribe();
    let mut pruned = false;
    loop {
        let changes = |s: &protocol::LibraryStatus| (s.revision, s.playlists);
        let seen = changes(&status.borrow_and_update());
        let planned = {
            let library = library.clone();
            tokio::task::spawn_blocking(move || library.download_plan()).await
        };
        let plan = match planned {
            Ok(Ok(plan)) => plan,
            Ok(Err(err)) => {
                library.set_download_status(0, Some(err.to_string()));
                Default::default()
            }
            Err(_) => return,
        };
        for path in &plan.remove {
            if let Err(err) = library.remove_download(path) {
                tracing::warn!(%err, path = %path.display(), "can't delete a download");
            }
        }
        if !pruned {
            let keep: Vec<_> = plan
                .fetch
                .iter()
                .filter_map(|w| source(&w.source))
                .collect();
            pairing.store().prune(&keep);
            pruned = true;
        }
        let mut error = None;
        for (done, wanted) in plan.fetch.iter().enumerate() {
            let waiting = (plan.fetch.len() - done) as u32;
            library.set_download_status(waiting, error.clone());
            match download(&library, &pairing, wanted).await {
                Ok(path) => tracing::info!(path = %path.display(), "downloaded"),
                Err(err) => {
                    tracing::info!(%err, source = wanted.source, "not downloaded");
                    error = Some(err);
                }
            }
            // Its members changed meanwhile (or it was scanned in): plan again.
            if changes(&status.borrow()) != seen {
                break;
            }
        }
        let waiting = match error {
            Some(_) => plan.fetch.len() as u32,
            None => 0,
        };
        library.set_download_status(waiting, error);
        tokio::select! {
            changed = status.wait_for(|s| changes(s) != seen) => {
                if changed.is_err() {
                    return;
                }
            }
            () = tokio::time::sleep(RETRY) => {}
        }
    }
}

/// The linked engine and its id for the catalog row `source`.
fn source(source: &str) -> Option<(String, i64)> {
    let (key, id) = source.strip_prefix("remote:")?.split_once(':')?;
    Some((key.into(), id.parse().ok()?))
}

/// Downloads `wanted` into the download folder: where it went.
async fn download(
    library: &Library,
    pairing: &Pairing,
    wanted: &Wanted,
) -> Result<PathBuf, String> {
    let track = TrackRef {
        source: "local".into(),
        id: wanted.source.clone(),
        keys: TrackKeys {
            audio_hash: wanted.hash.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let copies: Vec<_> = (library.copies(&track).into_iter())
        .filter(|(key, _)| pairing.is_linked(key))
        .collect();
    if copies.is_empty() {
        return Err("no linked engine has it".into());
    }
    // As it would come, so a stream of the original being fetched is shared.
    let quality = match transcode::plan(
        wanted.codec.as_deref(),
        wanted.bitrate,
        pairing.download_quality(),
    ) {
        Some(_) => pairing.download_quality(),
        None => Quality::Original,
    };
    let tags = library.download_tags(&wanted.source);
    let (source, told) = (wanted.source.clone(), library.clone());
    let tell = move |share: f32| {
        let percent = (share * 100.0) as u8;
        let source = source.clone();
        told.set_download_fetching(Some(Fetching { source, percent }));
    };
    tell(0.0);
    let fetched = files::fetch_whole(pairing, &copies, quality, &tags, wanted.length, tell).await;
    library.set_download_fetching(None);
    let (fetched, encoded) = fetched?;
    let ext = match encoded {
        true => "opus".into(),
        false => wanted.extension(),
    };
    let to = free(&library.download_folder().join(&wanted.name), &ext);
    let (library, wanted, store) = (library.clone(), wanted.clone(), pairing.store());
    tokio::task::spawn_blocking(move || {
        let saved = match encoded {
            true => place(&fetched, &to),
            // The original's file may be open for playing too: it stays for that.
            false => share(&fetched, &to),
        };
        saved.map_err(|err| match err.kind() {
            std::io::ErrorKind::StorageFull => "the disk is full".to_string(),
            _ => format!("can't save {}: {err}", to.display()),
        })?;
        match encoded {
            true => {
                let _ = std::fs::remove_file(fetched.with_extension("have"));
            }
            false => store.release(&fetched),
        }
        library
            .add_download(&to, &wanted)
            .map_err(|e| e.to_string())?;
        Ok(to)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// `name.ext`, or `name (2).ext` and so on if that is taken.
fn free(name: &Path, ext: &str) -> PathBuf {
    let with = |suffix: String| {
        let mut file = name.as_os_str().to_owned();
        file.push(format!("{suffix}.{ext}"));
        PathBuf::from(file)
    };
    (1..)
        .map(|n| {
            with(if n == 1 {
                String::new()
            } else {
                format!(" ({n})")
            })
        })
        .find(|path| !path.exists())
        .unwrap_or_else(|| with(String::new()))
}

/// Copies `from` to `to`, by a hard link on the same disk.
fn share(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::hard_link(from, to).is_ok() {
        return Ok(());
    }
    let copied = std::fs::copy(from, to).map(|_| ());
    if copied.is_err() {
        let _ = std::fs::remove_file(to);
    }
    copied
}

/// Moves `from` to `to`: renamed on the same disk, else copied.
fn place(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    let copied = std::fs::copy(from, to).and_then(|_| std::fs::remove_file(from));
    if copied.is_err() {
        let _ = std::fs::remove_file(to);
    }
    copied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::tests::{library, until, wav};
    use protocol::{Downloadable, LibraryQuery, PairingCommand, QualityFor};

    /// A, holding `files`, and B, linked to it with A's catalog and B's
    /// downloads running: B's library, the pairing, the folders.
    async fn linked(name: &str, files: &[(&str, Vec<u8>)]) -> (Library, Pairing, Vec<PathBuf>) {
        let (a, b) = (Pairing::in_memory(), Pairing::in_memory());
        for p in [&a, &b] {
            p.command(PairingCommand::SetInternet { allowed: false })
                .unwrap();
        }
        let (library_a, dir_a) = library(&format!("{name}-a"), files).await;
        let dir_b = std::env::temp_dir().join(format!("motif-dl-{name}-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir_b);
        let library_b = Library::open(&dir_b).unwrap();
        let folder = dir_b.join("Music/Motif").to_string_lossy().into_owned();
        library_b
            .query(LibraryQuery::SetDownloadFolder { path: folder })
            .unwrap();
        // Hashed, so the catalog carries the audio hash.
        until("hashed", || {
            library_a.status().hashing == 0 && !library_a.status().scanning
        })
        .await;
        library_a.set_device(a.engine_key()).unwrap();
        library_b.set_device(b.engine_key()).unwrap();
        b.link("A", a.engine_key(), &[]).unwrap();
        tokio::spawn(crate::links::run(
            b.clone(),
            0,
            Some(library_b.clone()),
            None,
        ));
        until("B bound", || b.iroh_port().is_some()).await;
        let at_b = format!("127.0.0.1:{}", b.iroh_port().unwrap());
        a.link("B", b.engine_key(), &[at_b]).unwrap();
        tokio::spawn(crate::links::run(
            a.clone(),
            0,
            Some(library_a.clone()),
            None,
        ));
        let want = files.len();
        until("A's catalog on B", || {
            crate::files::tests::tracks(&library_b).len() == want
        })
        .await;
        tokio::spawn(run(library_b.clone(), b.clone()));
        (library_b, b, vec![dir_a, dir_b])
    }

    /// The copies Motif downloaded (not their covers).
    fn downloaded(library: &Library) -> Vec<PathBuf> {
        let audio = |p: &PathBuf| p.extension().is_some_and(|e| e == "wav" || e == "opus");
        walk(&library.download_folder())
            .into_iter()
            .filter(audio)
            .collect()
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .flat_map(|e| match e.path().is_dir() {
                true => walk(&e.path()),
                false => vec![e.path()],
            })
            .collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_downloaded_track_plays_from_here_and_goes_when_taken_out() {
        let file = wav(3);
        let (library, pairing, dirs) = linked("track", &[("a.wav", file.clone())]).await;
        // The original, so it can be compared.
        pairing
            .command(PairingCommand::SetQuality {
                of: QualityFor::Download,
                quality: Quality::Original,
            })
            .unwrap();
        let remote = crate::files::tests::tracks(&library).remove(0);
        let member = Downloadable::Track {
            path: remote.id.clone(),
        };
        let set = |downloaded| LibraryQuery::SetDownloaded {
            member: member.clone(),
            downloaded,
        };
        library.query(set(true)).unwrap();
        until("downloaded", || downloaded(&library).len() == 1).await;
        let copy = downloaded(&library).remove(0);
        assert_eq!(std::fs::read(&copy).unwrap(), file);
        // Scanned in with the source's audio, so it plays from here.
        until("scanned in", || {
            library
                .find(&remote)
                .is_some_and(|p| p.starts_with(library.download_folder()))
        })
        .await;
        let status = library.status();
        assert_eq!(status.downloads.waiting, 0);
        assert_eq!(status.downloads.error, None);
        assert_eq!(status.downloads.fetching, None);

        library.query(set(false)).unwrap();
        until("deleted", || !copy.exists()).await;
        assert!(
            library
                .find(&remote)
                .is_none_or(|p| p.to_string_lossy().starts_with("remote:"))
        );
        for dir in dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_track_being_played_downloads_again_after_it_was_taken_out() {
        let file = wav(3);
        let (library, pairing, dirs) = linked("shared", &[("a.wav", file.clone())]).await;
        pairing
            .command(PairingCommand::SetQuality {
                of: QualityFor::Download,
                quality: Quality::Original,
            })
            .unwrap();
        let remote = crate::files::tests::tracks(&library).remove(0);
        // Open for playing, as the player holds the current and next tracks.
        let copies = library.copies(&remote);
        let source =
            crate::files::source(pairing.clone(), copies, tokio::runtime::Handle::current());
        let engine::Source::Stream { open, .. } = &source else {
            panic!("not a stream");
        };
        let open = open.clone();
        let playing = tokio::task::spawn_blocking(move || open().map_err(|e| e.to_string()))
            .await
            .unwrap()
            .unwrap();
        let set = |downloaded| LibraryQuery::SetDownloaded {
            member: Downloadable::Track {
                path: remote.id.clone(),
            },
            downloaded,
        };
        library.query(set(true)).unwrap();
        until("downloaded", || downloaded(&library).len() == 1).await;
        let copy = downloaded(&library).remove(0);
        library.query(set(false)).unwrap();
        until("deleted", || !copy.exists()).await;
        // Still playing: the download shares that fetch again.
        library.query(set(true)).unwrap();
        until("downloaded again", || downloaded(&library).len() == 1).await;
        assert_eq!(std::fs::read(&downloaded(&library)[0]).unwrap(), file);
        assert_eq!(library.status().downloads.error, None);
        drop(playing);
        for dir in dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn off_the_original_quality_a_lossless_track_downloads_as_opus_with_its_audio() {
        let (library, _pairing, dirs) = linked("opus", &[("a.wav", wav(3))]).await;
        let remote = crate::files::tests::tracks(&library).remove(0);
        let member = Downloadable::Track {
            path: remote.id.clone(),
        };
        library
            .query(LibraryQuery::SetDownloaded {
                member,
                downloaded: true,
            })
            .unwrap();
        until("downloaded", || downloaded(&library).len() == 1).await;
        assert_eq!(downloaded(&library)[0].extension().unwrap(), "opus");
        // The copy shares the source's audio hash: one track, played from here.
        let copy = downloaded(&library)[0].to_string_lossy().into_owned();
        let hash = || {
            let paths = vec![copy.clone()];
            let Ok(protocol::QueryResult::Tracks { tracks, .. }) =
                library.query(LibraryQuery::Lookup { paths })
            else {
                return None;
            };
            tracks.first()?.track.keys.audio_hash.clone()
        };
        until("hashed", || hash().is_some()).await;
        assert!(remote.keys.audio_hash.is_some());
        assert_eq!(hash(), remote.keys.audio_hash);
        assert_eq!(library.find(&remote).unwrap().to_string_lossy(), copy);
        for dir in dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
