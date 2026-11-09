//! Live play contexts: what a play query played from (a playlist, or a
//! search + sort), re-read when the library or playlists change and sent to
//! the engine as [`Command::UpdateContext`], so additions get played. The
//! source lives here, not in the engine, which only plays lists.

use std::{sync::Arc, time::Duration};

use engine::{Engine, EngineError};
use library::Library;
use protocol::{Command, LibraryQuery, LibraryStatus};
use tokio::sync::Mutex;

/// Least time between re-reads, so a scan (a revision every batch) re-reads
/// a 50k-track context every few seconds rather than on every batch.
const THROTTLE: Duration = Duration::from_secs(5);

/// Sends commands that replace the context, remembering the play query each
/// came from. Cheap to clone.
#[derive(Clone)]
pub(crate) struct Live {
    engine: Engine,
    source: Arc<Mutex<Option<Source>>>,
}

// shortcut: kept in memory only, so after an engine restart the restored context is a snapshot; save it with the playback file if that matters (Android).
// shortcut: an entry removed from the context by hand comes back on the source's next change; diff against the last read if users notice.
struct Source {
    query: LibraryQuery,
    /// What the tracks were read at (see [`stamp`]).
    stamp: (u64, u64),
}

impl Live {
    pub(crate) fn new(engine: Engine) -> Self {
        Self {
            engine,
            source: Arc::default(),
        }
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Sends `command` to the engine. A context it replaces stops being followed.
    pub(crate) async fn send(&self, command: Command) -> Result<(), EngineError> {
        if !matches!(command, Command::SetContext { .. }) {
            return self.engine.send(command).await;
        }
        let mut source = self.source.lock().await;
        self.engine.send(command).await?;
        *source = None;
        Ok(())
    }

    /// Plays what the play `query` selects and follows it from now on.
    pub(crate) async fn play(&self, library: &Library, query: LibraryQuery) -> Result<(), String> {
        let mut source = self.source.lock().await;
        let stamp = stamp(&library.status(), &query);
        let (lib, q) = (library.clone(), query.clone());
        let command = tokio::task::spawn_blocking(move || lib.selection(&q))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        self.engine.send(command).await.map_err(|e| e.to_string())?;
        *source = Some(Source { query, stamp });
        Ok(())
    }

    /// Re-reads the followed source whenever what it reads from changes, at
    /// most once every [`THROTTLE`]. Runs until the library goes.
    pub(crate) async fn follow(self, library: Library) {
        let mut status = library.subscribe();
        while status.changed().await.is_ok() {
            let now = status.borrow_and_update().clone();
            // The lock is held over the read so nothing else replaces the context meanwhile.
            let mut held = self.source.lock().await;
            let Some(source) = held.as_mut() else {
                continue;
            };
            let stamp = stamp(&now, &source.query);
            if stamp == source.stamp {
                continue;
            }
            let (lib, query) = (library.clone(), source.query.clone());
            match tokio::task::spawn_blocking(move || lib.selection(&query)).await {
                Ok(Ok(Command::SetContext { tracks, .. })) => {
                    if let Err(err) = self.engine.send(Command::UpdateContext { tracks }).await {
                        tracing::warn!(%err, "can't update the context");
                    }
                }
                // E.g. the playlist was deleted: what plays stays as it is.
                Ok(Err(err)) => tracing::debug!(%err, "can't re-read the context"),
                Ok(Ok(_)) | Err(_) => {}
            }
            source.stamp = stamp;
            drop(held);
            tokio::time::sleep(THROTTLE).await;
        }
    }
}

/// The library revision and, for a playlist, the playlists' revision: what
/// a source's tracks depend on.
fn stamp(status: &LibraryStatus, query: &LibraryQuery) -> (u64, u64) {
    match query {
        LibraryQuery::PlayPlaylist { .. } => (status.revision, status.playlists),
        _ => (status.revision, 0),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use engine::{Playable, Resolver};
    use protocol::TrackRef;

    use super::*;

    /// Plays nothing aloud: every track is simulated.
    struct Silent;

    impl Resolver for Silent {
        fn resolve(&self, _: &TrackRef) -> Result<Playable, String> {
            Ok(Playable::Simulated)
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "motif-live-{name}-{}-{}",
            std::process::id(),
            engine::wall_clock_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn copy_album(album: &str, to: &Path) {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../library/tests/fixtures")
            .join(album);
        let to = to.join(album);
        std::fs::create_dir_all(&to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
        }
    }

    async fn wait_until(what: &str, done: impl Fn() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        while !done() {
            assert!(tokio::time::Instant::now() < deadline, "timed out: {what}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    fn context_len(engine: &Engine) -> u32 {
        engine.state().context.map_or(0, |c| c.len)
    }

    fn current(engine: &Engine) -> u64 {
        engine.state().current.unwrap().id
    }

    #[tokio::test]
    async fn a_playing_playlist_or_library_follows_its_changes() {
        let (data, music) = (temp_dir("data"), temp_dir("music"));
        copy_album("Artist A/First Album", &music);
        let library = Library::open(&data).unwrap();
        let wait_scan = |tracks| {
            let library = library.clone();
            move || {
                let s = library.status();
                s.tracks == tracks && !s.scanning && s.hashing == 0
            }
        };
        let root = music.to_str().unwrap().to_string();
        library.query(LibraryQuery::AddRoot { path: root }).unwrap();
        wait_until("scanned", wait_scan(2)).await;
        let engine = Engine::spawn(Silent);
        let live = Live::new(engine.clone());
        tokio::spawn(live.clone().follow(library.clone()));
        let paths = |library: &Library| {
            let query = LibraryQuery::Tracks {
                search: None,
                sort: vec![],
                offset: 0,
                limit: 10,
            };
            let Ok(protocol::QueryResult::Tracks { tracks, .. }) = library.query(query) else {
                panic!()
            };
            tracks.into_iter().map(|t| t.track.id).collect::<Vec<_>>()
        };
        let all = paths(&library);

        let create = LibraryQuery::CreatePlaylist {
            id: "p".into(),
            name: "P".into(),
            paths: vec![all[0].clone()],
        };
        library.query(create).unwrap();
        let play = LibraryQuery::PlayPlaylist {
            id: "p".into(),
            start: 0,
        };
        live.play(&library, play).await.unwrap();
        engine.send(Command::Pause).await.unwrap();
        let playing = current(&engine);
        let add = LibraryQuery::AddToPlaylist {
            id: "p".into(),
            op: "o1".into(),
            paths: vec![all[1].clone()],
        };
        library.query(add).unwrap();
        wait_until("the playlist's new track", || context_len(&engine) == 2).await;
        assert_eq!(current(&engine), playing, "the current entry stays");

        let play = LibraryQuery::Play {
            search: None,
            sort: vec![],
            start: 0,
        };
        live.play(&library, play).await.unwrap();
        engine.send(Command::Pause).await.unwrap();
        let playing = current(&engine);
        copy_album("Artist B/Second Album", &music);
        library.query(LibraryQuery::Rescan).unwrap();
        wait_until("rescanned", wait_scan(4)).await;
        wait_until("the scanned album", || context_len(&engine) == 4).await;
        assert_eq!(current(&engine), playing);

        // Something else played: no longer followed.
        let other = Command::SetContext {
            tracks: vec![TrackRef::default()],
            start: None,
            kind: protocol::ContextKind::List,
            label: None,
        };
        live.send(other).await.unwrap();
        let remove = LibraryQuery::RemoveRoot {
            path: music.to_str().unwrap().into(),
        };
        library.query(remove).unwrap();
        tokio::time::sleep(THROTTLE + Duration::from_millis(500)).await;
        assert_eq!(context_len(&engine), 1);
    }
}
