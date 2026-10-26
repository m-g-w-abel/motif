//! The playback engine.
//!
//! [`Engine::spawn`] starts an actor task that owns all player state. Clients
//! talk to it through a cloneable [`Engine`] handle: commands go in over a
//! channel, and state snapshots come out through `watch` channels: one for
//! the transport and one for the queue, which changes rarely but can be big.
//!
//! Phase 0 has no audio output yet; [`Player`] advances position on a clock so
//! the protocol and UIs can be built against realistic behaviour.

mod player;
mod resolve;

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use protocol::{Command, PlayerState, QueueItem};
use tokio::sync::{mpsc, oneshot, watch};

pub use player::Player;
pub use resolve::{LocalFiles, Playable, Resolver};

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
    queue: watch::Receiver<Vec<QueueItem>>,
}

/// How often the engine checks for the end of the current track.
const TICK: Duration = Duration::from_millis(250);

impl Engine {
    /// Starts the engine on the current Tokio runtime. `resolver` decides what
    /// on this device plays each queue entry.
    // shortcut: resolving runs on the engine task, fine for local files; streaming sources (phase 8) need it async.
    pub fn spawn(resolver: impl Resolver) -> Self {
        let (requests, mut rx) = mpsc::channel::<Request>(64);
        let mut player = Player::default();
        let (state_tx, state) = watch::channel(snapshot(&player));
        let (queue_tx, queue) = watch::channel(Vec::new());

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
                    player.resolve_current(&resolver, Instant::now());
                    // Queue first, so a state never points into a queue
                    // subscribers haven't seen.
                    queue_tx.send_if_modified(|queue| {
                        let modified = queue.as_slice() != player.queue();
                        if modified {
                            *queue = player.queue().to_vec();
                        }
                        modified
                    });
                    state_tx.send_replace(snapshot(&player));
                }
            }
            tracing::debug!("engine stopped");
        });

        Self {
            requests,
            state,
            queue,
        }
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

    /// A receiver that is notified when the queue changes.
    pub fn subscribe_queue(&self) -> watch::Receiver<Vec<QueueItem>> {
        self.queue.clone()
    }
}

fn snapshot(player: &Player) -> PlayerState {
    player.snapshot(Instant::now(), wall_clock_ms())
}

/// The clock `PlayerState::sampled_at_ms` is measured on (Unix ms).
pub fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{PlaybackStatus, TrackRef};

    #[tokio::test]
    async fn commands_update_published_state() {
        let engine = Engine::spawn(LocalFiles);
        let mut updates = engine.subscribe();

        engine
            .send(Command::Enqueue {
                tracks: vec![TrackRef {
                    source: "demo".into(),
                    id: "/a.flac".into(),
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
        assert_eq!(engine.subscribe_queue().borrow().len(), 1);
    }

    #[tokio::test]
    async fn queue_is_published_only_when_it_changes() {
        let engine = Engine::spawn(LocalFiles);
        let mut queue = engine.subscribe_queue();
        let mut states = engine.subscribe();
        engine
            .send(Command::Enqueue {
                tracks: vec![TrackRef::default()],
            })
            .await
            .unwrap();
        queue.changed().await.unwrap();
        states.changed().await.unwrap();
        states.mark_unchanged();
        engine
            .send(Command::SetVolume { volume: 0.5 })
            .await
            .unwrap();
        // The state goes out after the queue would have.
        states.changed().await.unwrap();
        assert!(!queue.has_changed().unwrap());
    }

    #[tokio::test]
    async fn rejected_commands_return_errors() {
        let engine = Engine::spawn(LocalFiles);
        assert_eq!(
            engine.send(Command::PlayItem { item_id: 42 }).await,
            Err(EngineError::UnknownItem(42))
        );
    }
}
