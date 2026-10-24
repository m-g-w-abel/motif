//! Wire protocol between the engine and its clients.
//!
//! Every UI (desktop window, Android app, browser) talks to the engine with
//! these types, serialised as JSON over a WebSocket. TypeScript bindings are
//! generated into `apps/ui/src/lib/protocol/` by `cargo test -p protocol`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped on any incompatible change to the types below.
pub const PROTOCOL_VERSION: u32 = 3;

/// A message sent from a client to the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClientMessage {
    /// Echoed back in the matching [`ServerMessage::Response`].
    pub id: u32,
    pub command: Command,
}

/// A message sent from the engine to a client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum ServerMessage {
    /// Sent once, immediately after connecting.
    Hello {
        protocol_version: u32,
        engine_version: String,
        /// Engine wall clock (Unix ms), so clients can convert
        /// [`PlayerState::sampled_at_ms`] to their own clock.
        #[ts(type = "number")]
        now_ms: u64,
    },
    /// The outcome of the [`ClientMessage`] with the same `id`.
    Response { id: u32, error: Option<String> },
    /// Pushed whenever engine state changes.
    Event { event: Event },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Command {
    Play,
    Pause,
    TogglePlayback,
    Stop,
    Next,
    Previous,
    Seek {
        #[ts(type = "number")]
        position_ms: u64,
    },
    /// Linear gain in `0.0..=1.0`.
    SetVolume {
        volume: f32,
    },
    SetRepeat {
        mode: RepeatMode,
    },
    Enqueue {
        tracks: Vec<TrackInfo>,
    },
    /// Replaces the whole queue, e.g. when playing a track from a list.
    /// Playback starts at index `start` if given; otherwise the queue is
    /// left stopped.
    SetQueue {
        tracks: Vec<TrackInfo>,
        start: Option<u32>,
    },
    /// Jump to a queue entry by its [`QueueItem::id`].
    PlayItem {
        #[ts(type = "number")]
        item_id: u64,
    },
    RemoveItem {
        #[ts(type = "number")]
        item_id: u64,
    },
    ClearQueue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    State {
        state: PlayerState,
    },
    /// The whole queue, sent only when it changes (it can hold thousands of
    /// tracks) and always before the [`Event::State`] that refers to it.
    Queue {
        items: Vec<QueueItem>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PlaybackStatus {
    #[default]
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

/// Snapshot of the transport. The queue comes separately in [`Event::Queue`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlayerState {
    pub status: PlaybackStatus,
    pub volume: f32,
    pub repeat: RepeatMode,
    /// Position at the moment this snapshot was taken.
    #[ts(type = "number")]
    pub position_ms: u64,
    /// Engine wall clock (Unix ms) when `position_ms` was sampled, so clients
    /// can extrapolate the position without polling.
    #[ts(type = "number")]
    pub sampled_at_ms: u64,
    /// Index into the queue of the current track.
    pub current: Option<u32>,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            volume: 1.0,
            repeat: RepeatMode::Off,
            position_ms: 0,
            sampled_at_ms: 0,
            current: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueItem {
    /// Stable for the lifetime of the entry, unlike its index.
    #[ts(type = "number")]
    pub id: u64,
    pub track: TrackInfo,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackInfo {
    /// `file:///…`, or a source-plugin URI such as `youtube:…`.
    pub uri: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_wire_format() {
        let msg: ClientMessage =
            serde_json::from_str(r#"{"id":7,"command":{"type":"seek","position_ms":1500}}"#)
                .unwrap();
        assert_eq!(
            msg,
            ClientMessage {
                id: 7,
                command: Command::Seek { position_ms: 1500 }
            }
        );

        let unit: Command = serde_json::from_str(r#"{"type":"toggle_playback"}"#).unwrap();
        assert_eq!(unit, Command::TogglePlayback);
    }

    #[test]
    fn server_message_wire_format() {
        let json = serde_json::to_value(ServerMessage::Response { id: 3, error: None }).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "response", "id": 3, "error": null})
        );
    }
}
