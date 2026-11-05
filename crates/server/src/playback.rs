//! Where playback was (context, queue, current track and position), kept in
//! a JSON file next to the engine settings so a restarted engine comes back
//! to it, paused. Android kills backgrounded apps often.

use std::{path::Path, path::PathBuf, time::Duration};

use engine::{Engine, Playback};
use protocol::PlaybackStatus;

/// How often the position is saved while playing.
const WHILE_PLAYING: Duration = Duration::from_secs(5);
/// Least time between saves, so bursts of changes (a volume drag) save once.
const GAP: Duration = Duration::from_secs(1);

/// Restores the playback saved in `file`, then saves it after every change
/// and every few seconds while playing. A missing or broken file starts empty.
pub async fn keep(engine: Engine, file: PathBuf) {
    if let Some(saved) = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Playback>(&b).ok())
        && engine.restore(saved).await.is_err()
    {
        return;
    }
    let mut states = engine.subscribe();
    let mut queue = engine.subscribe_queue();
    let mut context = engine.subscribe_context();
    let mut last = None;
    loop {
        // Not borrow_and_update: a change during the last gap still counts.
        let playing = states.borrow().status == PlaybackStatus::Playing;
        let open = tokio::select! {
            r = states.changed() => r.is_ok(),
            r = queue.changed() => r.is_ok(),
            r = context.changed() => r.is_ok(),
            _ = tokio::time::sleep(WHILE_PLAYING), if playing => true,
        };
        if !open {
            break;
        }
        let Ok(now) = engine.playback().await else {
            break;
        };
        if last.as_ref() != Some(&now) {
            if let Err(err) = save(&file, &now) {
                tracing::warn!(%err, "can't save playback");
            }
            last = Some(now);
        }
        tokio::time::sleep(GAP).await;
    }
}

/// Writes a temporary file and renames it over `file`, so a kill mid-write
/// leaves the old one whole.
fn save(file: &Path, playback: &Playback) -> std::io::Result<()> {
    let temp = file.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec(playback)?)?;
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

    /// Waits until `file` contains `text`.
    async fn wait_for(file: &Path, text: &str) {
        for _ in 0..80 {
            if std::fs::read_to_string(file).is_ok_and(|s| s.contains(text)) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("{} never contained {text}", file.display());
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
        let file = dir("roundtrip").join("playback.json");
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
        wait_for(&file, r#""position_ms":7000"#).await;

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
            wait_for(&file, "/x.flac").await;
            assert_eq!(engine.state().status, PlaybackStatus::Stopped);
            assert_eq!(
                engine.subscribe_queue().borrow().len(),
                1,
                "nothing restored"
            );
        }
    }
}
