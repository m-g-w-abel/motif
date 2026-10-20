//! The playback engine.
//!
//! [`Engine::spawn`] starts an actor task that owns all player state. Clients
//! talk to it through a cloneable [`Engine`] handle: commands go in over a
//! channel, and state snapshots come out through a `watch` channel.
//!
//! Phase 0 has no audio output yet; [`Player`] advances position on a clock so
//! the protocol and UIs can be built against realistic behaviour.

mod player;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use protocol::{Command, PlayerState};
use tokio::sync::{mpsc, oneshot, watch};

pub use player::Player;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EngineError {
    #[error("no queue item with id {0}")]
    UnknownItem(u64),
    #[error("queue index {0} is out of range")]
    IndexOutOfRange(u32),
    #[error("volume must be between 0 and 1, got {0}")]
    InvalidVolume(f32),
    #[error("the engine has shut down")]
    Stopped,
}

struct Request {
    command: Command,
    reply: oneshot::Sender<Result<(), EngineError>>,
}

/// Handle to a running engine. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    requests: mpsc::Sender<Request>,
    state: watch::Receiver<PlayerState>,
}

/// How often the engine checks for the end of the current track.
const TICK: Duration = Duration::from_millis(250);

impl Engine {
    /// Starts the engine on the current Tokio runtime.
    pub fn spawn() -> Self {
        let (requests, mut rx) = mpsc::channel::<Request>(64);
        let mut player = Player::default();
        let (state_tx, state) = watch::channel(snapshot(&player));

        tokio::spawn(async move {
            let mut tick = tokio::time::interval(TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let changed = tokio::select! {
                    request = rx.recv() => {
                        let Some(Request { command, reply }) = request else { break };
                        tracing::debug!(?command, "engine command");
                        let result = player.apply(command, Instant::now());
                        let changed = result.is_ok();
                        let _ = reply.send(result);
                        changed
                    }
                    _ = tick.tick() => player.tick(Instant::now()),
                };
                if changed {
                    state_tx.send_replace(snapshot(&player));
                }
            }
            tracing::debug!("engine stopped");
        });

        Self { requests, state }
    }

    /// Applies a command and waits for the engine to accept or reject it.
    pub async fn send(&self, command: Command) -> Result<(), EngineError> {
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request { command, reply })
            .await
            .map_err(|_| EngineError::Stopped)?;
        response.await.map_err(|_| EngineError::Stopped)?
    }

    /// The most recent state snapshot.
    pub fn state(&self) -> PlayerState {
        self.state.borrow().clone()
    }

    /// A receiver that is notified on every state change.
    pub fn subscribe(&self) -> watch::Receiver<PlayerState> {
        self.state.clone()
    }
}

fn snapshot(player: &Player) -> PlayerState {
    let wall_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    player.snapshot(Instant::now(), wall_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{PlaybackStatus, TrackInfo};

    #[tokio::test]
    async fn commands_update_published_state() {
        let engine = Engine::spawn();
        let mut updates = engine.subscribe();

        engine
            .send(Command::Enqueue {
                tracks: vec![TrackInfo {
                    uri: "file:///a.flac".into(),
                    ..Default::default()
                }],
            })
            .await
            .unwrap();
        engine.send(Command::Play).await.unwrap();

        updates.changed().await.unwrap();
        let state = engine.state();
        assert_eq!(state.status, PlaybackStatus::Playing);
        assert_eq!(state.current, Some(0));
    }

    #[tokio::test]
    async fn rejected_commands_return_errors() {
        let engine = Engine::spawn();
        assert_eq!(
            engine.send(Command::PlayItem { item_id: 42 }).await,
            Err(EngineError::UnknownItem(42))
        );
    }
}
