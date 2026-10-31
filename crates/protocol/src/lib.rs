//! Wire protocol between the engine and its clients.
//!
//! Every UI (desktop window, Android app, browser) talks to the engine with
//! these types, serialised as JSON over a WebSocket. TypeScript bindings are
//! generated into `apps/ui/src/lib/protocol/` by `cargo test -p protocol`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped on any incompatible change to the types below.
pub const PROTOCOL_VERSION: u32 = 7;

/// A message sent from a client: a command for the engine, a query for the
/// library, or a change of subscription. `id` is echoed back in the matching
/// [`ServerMessage::Response`] or [`ServerMessage::QueryResult`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub enum ClientMessage {
    Command {
        id: u32,
        command: Command,
    },
    Query {
        id: u32,
        query: LibraryQuery,
    },
    /// Starts (`true`) or stops [`Event::Bands`] for this connection. Off
    /// when connecting; the engine only analyses audio while someone listens.
    Subscribe {
        id: u32,
        bands: bool,
    },
    /// Replaces the host's [`IntegrationSettings`]; every client then gets
    /// [`Event::Integrations`].
    SetIntegrations {
        id: u32,
        integrations: IntegrationSettings,
    },
}

/// A message sent from the engine to a client.
// Built only to be serialised, so variant sizes don't matter.
#[allow(clippy::large_enum_variant)]
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
    /// The outcome of the [`ClientMessage::Command`] with the same `id`.
    Response { id: u32, error: Option<String> },
    /// The answer to the [`ClientMessage::Query`] with the same `id`.
    QueryResult {
        id: u32,
        error: Option<String>,
        result: Option<QueryResult>,
    },
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
    /// Adds tracks to the end of the queue.
    Enqueue {
        tracks: Vec<TrackRef>,
    },
    /// Adds tracks to the front of the queue, so they play next.
    EnqueueNext {
        tracks: Vec<TrackRef>,
    },
    /// Replaces the context with the list played from, and clears the queue.
    /// Playback starts at index `start` if given; otherwise it is left
    /// stopped. Library `Play`/`PlayAlbum` queries send this.
    SetContext {
        tracks: Vec<TrackRef>,
        start: Option<u32>,
        kind: ContextKind,
        /// E.g. the album or list name.
        label: Option<String>,
    },
    /// Plays a queue or context entry by its [`QueueItem::id`]. A queued
    /// entry leaves the queue; a context entry moves the context there. The
    /// rest of the queue is kept either way.
    PlayItem {
        #[ts(type = "number")]
        item_id: u64,
    },
    /// Removes a queue or context entry.
    RemoveItem {
        #[ts(type = "number")]
        item_id: u64,
    },
    /// Moves a queue entry to index `to` of the queue.
    MoveItem {
        #[ts(type = "number")]
        item_id: u64,
        to: u32,
    },
    /// Empties the queue. The context is kept.
    ClearQueue,
    /// Decoder plugins to try for each file, in order; plugins left out are
    /// not used. Names come from [`PlayerState::decoders`].
    SetDecoders {
        order: Vec<String>,
    },
    /// Plays through this output device ([`OutputDevice::id`]), or the
    /// system default if `None`.
    SetOutput {
        device: Option<String>,
    },
    /// Refreshes [`PlayerState::outputs`] (devices come and go).
    ListOutputs,
    SetReplayGain {
        settings: ReplayGainSettings,
    },
}

/// Which ReplayGain value levels each track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReplayGainMode {
    Off,
    Track,
    Album,
    /// Album gain while playing an album context, track gain otherwise.
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplayGainSettings {
    pub mode: ReplayGainMode,
    /// Added to tagged gains, in dB.
    pub preamp_db: f32,
    /// Lowers the gain so the tagged peak doesn't clip.
    pub prevent_clipping: bool,
    /// Applied instead to files without gain tags, in dB.
    pub untagged_db: f32,
}

impl Default for ReplayGainSettings {
    fn default() -> Self {
        Self {
            mode: ReplayGainMode::Auto,
            preamp_db: 0.0,
            prevent_clipping: true,
            untagged_db: 0.0,
        }
    }
}

/// A file's ReplayGain values: gains in dB relative to the ReplayGain
/// reference (-18 LUFS), peaks as linear sample amplitude.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReplayGain {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputDevice {
    /// Stable across restarts where the OS allows.
    pub id: String,
    pub name: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    State {
        state: PlayerState,
    },
    /// The queue (tracks added to play before the context resumes), sent
    /// when it changes.
    Queue {
        items: Vec<QueueItem>,
    },
    /// The context's entries from `offset`: all of them for up to 200, else a
    /// window around the position that moves every 100 tracks. Sent when it
    /// changes, and always before the [`Event::State`] that refers to it.
    Context {
        offset: u32,
        items: Vec<QueueItem>,
    },
    /// The library changed: scanning progress, or new contents (`revision`).
    Library {
        status: LibraryStatus,
    },
    /// Spectrum of what is playing, for visualisers: 32 log-spaced bands
    /// from 40 Hz to 16 kHz, each `0` (60 dB down or silent) to `255` (full
    /// scale), before the volume is applied. About 30 a second while playing,
    /// only after [`ClientMessage::Subscribe`].
    Bands {
        bands: Vec<u8>,
    },
    /// The host's integration settings, on connecting and whenever they change.
    Integrations {
        settings: IntegrationSettings,
    },
}

/// Settings for integrations with outside services (lyrics, scrobbling…),
/// kept and obeyed by the host and always sent whole. Secrets (tokens) stay
/// on the host; at most who is signed in goes over the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct IntegrationSettings {
    /// Look up lyrics on LRCLIB.
    pub lrclib: bool,
    /// Scrobble to ListenBrainz.
    pub listenbrainz: ScrobbleAccount,
    /// Set by the host, ignored from clients: no OS keyring was found, so
    /// secrets are kept unencrypted in a file only this user can read.
    pub secrets_unencrypted: bool,
    /// Show what plays as Discord Rich Presence (desktop hosts).
    pub discord: bool,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
            lrclib: true,
            listenbrainz: ScrobbleAccount::default(),
            secrets_unencrypted: false,
            discord: false,
        }
    }
}

/// A scrobbling service in [`IntegrationSettings`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct ScrobbleAccount {
    pub enabled: bool,
    /// Set by the host: the signed-in account.
    pub user: Option<String>,
    /// Set by the host: why signing in or the last submission failed.
    pub error: Option<String>,
    /// From clients only, never sent out or saved: a credential to sign in
    /// with (the ListenBrainz user token), or empty to sign out.
    #[serde(skip_serializing, default)]
    pub sign_in: Option<String>,
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

/// What kind of list the context is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ContextKind {
    Album,
    /// Any other list: the library, search results, later playlists.
    #[default]
    List,
}

/// The list playback came from. Its entries come in [`Event::Context`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlayContext {
    pub kind: ContextKind,
    pub label: Option<String>,
    /// Number of entries.
    pub len: u32,
    /// Index of the current track, if it comes from the context.
    pub index: Option<u32>,
    /// Index of the entry that plays once the queue is empty, if any
    /// (following repeat).
    pub next: Option<u32>,
}

/// Snapshot of the transport. The queue and context entries come separately
/// in [`Event::Queue`] and [`Event::Context`].
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
    /// The playing, paused or last played entry.
    pub current: Option<QueueItem>,
    /// `None` until something is played from a list.
    pub context: Option<PlayContext>,
    /// Enabled decoder plugins in the order they are tried.
    pub decoders: Vec<String>,
    /// Every decoder plugin this engine has.
    pub available_decoders: Vec<String>,
    /// The chosen output device, or `None` for the system default.
    pub output: Option<String>,
    /// Output devices found at the last [`Command::ListOutputs`].
    pub outputs: Vec<OutputDevice>,
    pub replay_gain: ReplayGainSettings,
    /// The ReplayGain adjustment applied to the current track, in dB
    /// (`None` when off or nothing is loaded).
    pub gain_db: Option<f32>,
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
            context: None,
            decoders: Vec::new(),
            available_decoders: Vec::new(),
            output: None,
            outputs: Vec::new(),
            replay_gain: ReplayGainSettings::default(),
            gain_db: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueueItem {
    /// Stable for the lifetime of the entry, unlike its index.
    #[ts(type = "number")]
    pub id: u64,
    pub track: TrackRef,
    /// Why nothing on the playing device could play this entry, if it was
    /// tried and failed. Such entries are skipped, not removed.
    pub error: Option<String>,
}

/// A track independent of where it is stored, so any device can play it with
/// whatever it has: its own copy first, then the original source, then a
/// match elsewhere (see the resolver in the engine).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackRef {
    /// Where `id` comes from: `local` (a file path on the device that queued
    /// it), later source plugins such as `youtube`.
    pub source: String,
    pub id: String,
    /// Identifies the recording across devices and sources.
    #[serde(default)]
    pub keys: TrackKeys,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<u64>,
}

/// Identity keys for matching a [`TrackRef`] to a copy on another device.
/// Tags (artist, title, album, duration) are the fallback when these are absent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackKeys {
    pub musicbrainz_recording: Option<String>,
    pub isrc: Option<String>,
    /// Hash of the decoded audio only, so tag edits don't change it.
    pub audio_hash: Option<String>,
}

/// What a library list can be ordered by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SortKey {
    AlbumArtist,
    Artist,
    Album,
    Year,
    /// When the track first entered the library.
    Added,
    Title,
    Genre,
    Disc,
    Track,
    Duration,
    Format,
    Path,
}

/// One tier of a list's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SortRule {
    pub key: SortKey,
    pub descending: bool,
}

/// Questions and requests for the library. Lists are paged so a large
/// library never crosses the wire at once. Each takes an optional search and
/// `sort`, tiered rules applied in turn (empty for the default: album artist,
/// album, disc, track); queries that should agree must send the same rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum LibraryQuery {
    Status,
    Tracks {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        offset: u32,
        limit: u32,
    },
    /// Runs of consecutive tracks from one album, for album headers.
    Groups {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
    },
    Albums {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        offset: u32,
        limit: u32,
    },
    /// Index of an album's first track in [`LibraryQuery::Tracks`].
    AlbumPosition {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        album: String,
    },
    /// Library details (artwork, format…) for tracks by path, e.g. the queue
    /// or the playing track. Paths not in the library are left out.
    Lookup {
        paths: Vec<String>,
    },
    /// Makes the matching tracks the context and plays from `start`.
    Play {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        start: u32,
    },
    PlayAlbum {
        album: String,
        #[serde(default)]
        sort: Vec<SortRule>,
    },
    /// The tracks of these albums, album after album in the order given,
    /// e.g. to queue them.
    AlbumTracks {
        albums: Vec<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
    },
    AddRoot {
        path: String,
    },
    RemoveRoot {
        path: String,
    },
    Rescan,
    /// Whether changes made outside Motif (files added, edited or removed by
    /// other programs) are picked up as they happen. Rescan picks them up
    /// either way.
    SetWatching {
        enabled: bool,
    },
    /// The seek bar waveform of the library track at `path`. Decoded on first
    /// request (up to a few seconds for a long file), then cached.
    Waveform {
        path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum QueryResult {
    Status {
        status: LibraryStatus,
    },
    Tracks {
        total: u32,
        offset: u32,
        tracks: Vec<LibraryTrack>,
    },
    Groups {
        groups: Vec<TrackGroup>,
    },
    Albums {
        total: u32,
        offset: u32,
        albums: Vec<LibraryAlbum>,
    },
    Position {
        index: Option<u32>,
    },
    /// Loudness (0–255, the loudest point 255) at evenly spaced points
    /// along the track.
    Waveform {
        levels: Vec<u8>,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryStatus {
    /// Folders being scanned.
    pub roots: Vec<String>,
    pub tracks: u32,
    /// Tracks whose files have disappeared. They stay in the library.
    pub missing: u32,
    /// See [`LibraryQuery::SetWatching`].
    pub watching: bool,
    pub scanning: bool,
    /// Files looked at so far in the current scan.
    pub scanned: u32,
    /// Tracks still waiting for their audio hash.
    pub hashing: u32,
    /// Bumped whenever the contents change, so clients refetch what they show.
    #[ts(type = "number")]
    pub revision: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryTrack {
    #[ts(type = "number")]
    pub id: i64,
    pub track: TrackRef,
    pub album_artist: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    /// Codec or container, e.g. `FLAC`, `MP3`.
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub channels: Option<u32>,
    /// kbps.
    pub bitrate: Option<u32>,
    /// Artwork id, served at `/art/<id>`.
    pub art: Option<String>,
    /// The file is no longer where the library last saw it.
    pub missing: bool,
    /// Why the file can't be played, if it is damaged.
    pub problem: Option<String>,
}

/// One of a track's images, as listed by the server's `GET /images`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackImage {
    /// Artwork key, for `/art/<key>`.
    pub key: String,
    /// What it shows, when its picture type or file name says; never guessed.
    pub kind: Option<ImageKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ImageKind {
    Front,
    Back,
    Booklet,
    Disc,
    Artist,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryAlbum {
    /// Album artist and title, as used by [`LibraryQuery::PlayAlbum`].
    pub key: String,
    pub title: String,
    pub artist: String,
    pub year: Option<u32>,
    pub tracks: u32,
    #[ts(type = "number")]
    pub duration_ms: u64,
    pub art: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackGroup {
    /// Index of the first track in [`LibraryQuery::Tracks`].
    pub start: u32,
    pub count: u32,
    pub key: String,
    pub title: String,
    pub artist: String,
    #[ts(type = "number")]
    pub duration_ms: u64,
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
            ClientMessage::Command {
                id: 7,
                command: Command::Seek { position_ms: 1500 }
            }
        );
        let query: ClientMessage =
            serde_json::from_str(r#"{"id":8,"query":{"type":"rescan"}}"#).unwrap();
        assert_eq!(
            query,
            ClientMessage::Query {
                id: 8,
                query: LibraryQuery::Rescan
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

    #[test]
    fn sign_in_credentials_are_never_sent_out() {
        let settings: IntegrationSettings =
            serde_json::from_str(r#"{"listenbrainz":{"sign_in":"t0ken"}}"#).unwrap();
        assert_eq!(settings.listenbrainz.sign_in.as_deref(), Some("t0ken"));
        assert!(settings.lrclib, "missing fields keep their defaults");
        let json = serde_json::to_string(&settings).unwrap();
        assert!(!json.contains("t0ken"));
    }
}
