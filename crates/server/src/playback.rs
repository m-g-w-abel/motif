//! Where playback was (context, queue, current track and position), kept in
//! a JSON file next to the engine settings so a restarted engine comes back
//! to it, paused. Android kills backgrounded apps often.

use std::{path::Path, path::PathBuf, time::Duration};

use engine::{Engine, Playback};
use protocol::{PlaybackStatus, PlayerState};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// How often the position is saved while playing.
const WHILE_PLAYING: Duration = Duration::from_secs(5);
/// Least time between saves, so bursts of changes (a volume drag) save once.
const GAP: Duration = Duration::from_secs(1);

/// The current entry's position, saved more often than the rest: with a big
/// context the whole file is megabytes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Position {
    item: u64,
    position_ms: u64,
}

impl Position {
    /// Where `state` is now, going by the engine clock.
    fn of(state: &PlayerState) -> Option<Self> {
        let current = state.current.as_ref()?;
        let mut position_ms = state.position_ms;
        if state.status == PlaybackStatus::Playing {
            position_ms += engine::wall_clock_ms().saturating_sub(state.sampled_at_ms);
        }
        if let Some(duration) = current.track.duration_ms {
            position_ms = position_ms.min(duration);
        }
        Some(Self {
            item: current.id,
            position_ms,
        })
    }
}

/// Restores the playback saved in `file` (with the position in a smaller
/// file beside it), then saves it after changes and every few seconds while
/// playing. A missing or broken file starts empty.
pub async fn keep(engine: Engine, file: PathBuf) {
    let position_file = file.with_file_name("playback-position.json");
    if let Some(mut saved) = read::<Playback>(&file) {
        if let Some(at) = read::<Position>(&position_file) {
            saved.seek(at.item, at.position_ms);
        }
        if engine.restore(saved).await.is_err() {
            return;
        }
    }
    let mut states = engine.subscribe();
    let mut queue = engine.subscribe_queue();
    let mut context = engine.subscribe_context();
    let mut lists_changed = true;
    let (mut last_lists, mut last_position) = (None, None);
    loop {
        // Not borrow_and_update: a change during the last gap still counts.
        let playing = states.borrow().status == PlaybackStatus::Playing;
        let open = tokio::select! {
            r = states.changed() => r.is_ok(),
            r = queue.changed() => { lists_changed = true; r.is_ok() }
            r = context.changed() => { lists_changed = true; r.is_ok() }
            _ = tokio::time::sleep(WHILE_PLAYING), if playing => true,
        };
        if !open {
            break;
        }
        let state = states.borrow_and_update().clone();
        // The lists, history and context position change only with these.
        let lists = (state.current.as_ref().map(|c| c.id), state.context.clone());
        if lists_changed || last_lists.as_ref() != Some(&lists) {
            // Seen before the snapshot, so a later edit still wakes the loop.
            queue.mark_unchanged();
            context.mark_unchanged();
            let Ok(playback) = engine.playback().await else {
                break;
            };
            write(&file, playback).await;
            (lists_changed, last_lists) = (false, Some(lists));
        }
        let position = Position::of(&state);
        if position != last_position
            && let Some(position) = position
        {
            write(&position_file, position).await;
            last_position = Some(position);
        }
        tokio::time::sleep(GAP).await;
    }
}

fn read<T: DeserializeOwned>(file: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// Writes off the async threads (the lists can take a second).
async fn write(file: &Path, value: impl Serialize + Send + 'static) {
    let file = file.to_path_buf();
    let saved = tokio::task::spawn_blocking(move || save(&file, &value)).await;
    if let Ok(Err(err)) = saved {
        tracing::warn!(%err, "can't save playback");
    }
}

/// Writes a temporary file and renames it over `file`, so a kill mid-write
/// leaves the old one whole.
fn save(file: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let temp = file.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(value)?)?;
    std::fs::rename(temp, file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{Command, ContextKind, TrackRef};

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("motif-playback-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Waits up to 8 s for `done`.
    async fn wait_until(what: &str, done: impl Fn() -> bool) {
        for _ in 0..80 {
            if done() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("never {what}");
    }

    fn contains(file: &Path, text: &str) -> bool {
        std::fs::read_to_string(file).is_ok_and(|s| s.contains(text))
    }

    fn saved_position(dir: &Path) -> u64 {
        read::<Position>(&dir.join("playback-position.json")).map_or(0, |p| p.position_ms)
    }

    fn demo(name: &str) -> TrackRef {
        TrackRef {
            source: "demo".into(),
            id: format!("/{name}.flac"),
            duration_ms: Some(60_000),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_new_engine_comes_back_paused_where_the_last_one_was() {
        let dir = dir("roundtrip");
        let file = dir.join("playback.json");
        let first = Engine::spawn(engine::LocalFiles);
        tokio::spawn(keep(first.clone(), file.clone()));
        let play = Command::SetContext {
            tracks: vec![demo("a"), demo("b")],
            start: Some(1),
            kind: ContextKind::Album,
            label: Some("Blue".into()),
        };
        first.send(play).await.unwrap();
        first
            .send(Command::Enqueue {
                tracks: vec![demo("x")],
            })
            .await
            .unwrap();
        first
            .send(Command::Seek { position_ms: 7_000 })
            .await
            .unwrap();
        first.send(Command::Pause).await.unwrap();
        wait_until("saved", || {
            contains(&file, "/x.flac") && saved_position(&dir) == 7_000
        })
        .await;

        let second = Engine::spawn(engine::LocalFiles);
        let mut states = second.subscribe();
        tokio::spawn(keep(second.clone(), file));
        states.changed().await.unwrap();
        let state = second.state();
        assert_eq!(state.status, PlaybackStatus::Paused);
        assert_eq!(state.current.unwrap().track.id, "/b.flac");
        assert_eq!(state.position_ms, 7_000);
        assert_eq!(state.context.unwrap().label.as_deref(), Some("Blue"));
        assert_eq!(second.subscribe_queue().borrow()[0].track.id, "/x.flac");
    }

    #[tokio::test]
    async fn while_playing_only_the_position_is_saved_again() {
        let dir = dir("position");
        let file = dir.join("playback.json");
        let first = Engine::spawn(engine::LocalFiles);
        tokio::spawn(keep(first.clone(), file.clone()));
        let play = Command::SetContext {
            tracks: vec![demo("a")],
            start: Some(0),
            kind: ContextKind::List,
            label: None,
        };
        first.send(play).await.unwrap();
        first
            .send(Command::Seek {
                position_ms: 10_000,
            })
            .await
            .unwrap();
        wait_until("saved", || saved_position(&dir) >= 10_000).await;
        let lists = std::fs::read(&file).unwrap();
        wait_until("saved again", || saved_position(&dir) >= 15_000).await;
        assert_eq!(
            std::fs::read(&file).unwrap(),
            lists,
            "the lists aren't rewritten"
        );

        let second = Engine::spawn(engine::LocalFiles);
        let mut states = second.subscribe();
        tokio::spawn(keep(second.clone(), file));
        states.changed().await.unwrap();
        assert!(second.state().position_ms >= 15_000);
    }

    #[tokio::test]
    async fn a_missing_or_broken_file_starts_empty() {
        let dir = dir("broken");
        std::fs::write(dir.join("broken.json"), b"{ not json").unwrap();
        for name in ["missing.json", "broken.json"] {
            let file = dir.join(name);
            let engine = Engine::spawn(engine::LocalFiles);
            tokio::spawn(keep(engine.clone(), file.clone()));
            let tracks = vec![demo("x")];
            engine.send(Command::Enqueue { tracks }).await.unwrap();
            // Saving works, so keep carried on past the bad file.
            wait_until("saved", || contains(&file, "/x.flac")).await;
            assert_eq!(engine.state().status, PlaybackStatus::Stopped);
            assert_eq!(
                engine.subscribe_queue().borrow().len(),
                1,
                "nothing restored"
            );
        }
    }
}
