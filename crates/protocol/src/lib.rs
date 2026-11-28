//! Wire protocol between the engine and its clients.
//!
//! Every UI (desktop window, Android app, browser) talks to the engine with
//! these types, serialised as JSON over a WebSocket. TypeScript bindings are
//! generated into `apps/ui/src/lib/protocol/` by `cargo test -p protocol`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bumped on any incompatible change to the types below.
pub const PROTOCOL_VERSION: u32 = 9;

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
    /// Changes the host's [`IntegrationSettings`]; every client then gets
    /// [`Event::Integrations`].
    SetIntegrations {
        id: u32,
        changes: Box<IntegrationChanges>,
    },
    /// Pairing and paired-device management; needs [`Scope::Pairing`].
    Pairing {
        id: u32,
        pairing: PairingCommand,
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
        /// What the engine is called ([`PairingState::name`]), which
        /// clients show it as.
        engine_name: String,
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
    /// Plays the context in a random order: what hasn't played yet is
    /// shuffled, the current track stays. Off goes back to the list's order
    /// from the current track. A new context while on is shuffled too, its
    /// `start` entry first.
    SetShuffle {
        on: bool,
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
    /// The list played from changed (a playlist edited, a scan): its tracks
    /// now, in order. Entries keep their ids by track; the current entry and
    /// history stay, removed tracks drop and new ones are added. Unshuffled the
    /// order follows `tracks`; shuffled, new tracks go at random among what
    /// hasn't played. The server sends this for live play queries.
    UpdateContext {
        tracks: Vec<TrackRef>,
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
    /// Whether a song whose device can't be reached is skipped
    /// ([`PlayerState::skip_unreachable`]).
    SetSkipUnreachable {
        skip: bool,
    },
    /// Plays the entry stopped on because its device couldn't be reached
    /// ([`Unreachable::stopped`]) again; does nothing otherwise.
    Retry,
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
    /// What this connection may do, on connecting and whenever it changes
    /// (the host changed the device's preset), so clients can hide controls.
    Access {
        scopes: Vec<Scope>,
    },
    /// Paired devices and pairing in progress, on connecting and whenever
    /// they change. Only sent to clients with [`Scope::Pairing`].
    Pairing {
        state: PairingState,
    },
}

/// Settings for integrations with outside services (lyrics, scrobbling…),
/// kept and obeyed by the host, always sent out whole and changed with
/// [`IntegrationChanges`]. Secrets (tokens) stay
/// on the host; at most who is signed in goes over the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct IntegrationSettings {
    /// Look up lyrics on LRCLIB.
    pub lrclib: bool,
    /// Scrobble to ListenBrainz.
    pub listenbrainz: ScrobbleAccount,
    /// Scrobble to Last.fm.
    pub lastfm: ScrobbleAccount,
    /// Scrobble to Libre.fm.
    pub librefm: ScrobbleAccount,
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
            lastfm: ScrobbleAccount::default(),
            librefm: ScrobbleAccount::default(),
            secrets_unencrypted: false,
            discord: false,
        }
    }
}

/// The [`IntegrationSettings`] a client changes: only the fields given are
/// replaced, so a client with an old copy doesn't undo what another client
/// (or the host) changed meanwhile. Of an account, only `enabled` and
/// `sign_in` are taken; the rest is the host's.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct IntegrationChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lrclib: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub listenbrainz: Option<ScrobbleAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lastfm: Option<ScrobbleAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub librefm: Option<ScrobbleAccount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub discord: Option<bool>,
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
    /// Set by the host while signing in through the browser (Last.fm, Libre.fm): the
    /// page where the user allows Motif.
    pub auth_url: Option<String>,
    /// From clients only, never sent out or saved: a credential to sign in
    /// with (the ListenBrainz user token; anything for Last.fm and Libre.fm, which sign
    /// in through the browser), or empty to sign out or stop signing in.
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
    pub shuffle: bool,
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
    /// How the current track comes from a linked engine, while it streams
    /// from one (`None` for this device's files).
    pub streamed: Option<Streamed>,
    /// A device that couldn't be reached, while it matters: playback stopped
    /// on its song, or songs were skipped past it.
    pub unreachable: Option<Unreachable>,
    /// Skip songs whose device can't be reached rather than stop on them.
    pub skip_unreachable: bool,
}

/// Songs a device held that couldn't be played because it couldn't be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Unreachable {
    /// The device's name.
    pub from: String,
    /// Songs skipped past ([`PlayerState::skip_unreachable`]).
    pub skipped: u32,
    /// Playback stopped on the current entry, its song; [`Command::Retry`]
    /// plays it again.
    pub stopped: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            status: PlaybackStatus::Stopped,
            volume: 1.0,
            repeat: RepeatMode::Off,
            shuffle: false,
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
            streamed: None,
            unreachable: None,
            skip_unreachable: false,
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
    /// Favourites first when descending.
    Favourite,
    /// Half stars; unrated tracks count as none.
    Rating,
}

/// Which tracks a list holds besides its search: both limits apply.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TrackFilter {
    /// Only favourites.
    #[serde(default)]
    pub favourite: bool,
    /// Only tracks rated at least this many half stars (1 to 10).
    #[serde(default)]
    #[ts(optional)]
    pub min_rating: Option<u8>,
    /// Only tracks whose audio is on this engine, not linked engines'.
    #[serde(default)]
    pub this_device: bool,
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
        #[serde(default)]
        filter: TrackFilter,
        offset: u32,
        limit: u32,
    },
    /// Runs of consecutive tracks from one album, for album headers.
    Groups {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        #[serde(default)]
        filter: TrackFilter,
    },
    Albums {
        search: Option<String>,
        /// Only this album artist's albums (a track without one counts under its artist).
        #[serde(default)]
        #[ts(optional)]
        artist: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        /// Albums with a track it lets through, counted by those.
        #[serde(default)]
        filter: TrackFilter,
        offset: u32,
        limit: u32,
    },
    /// Artists whose name matches `search` (as [`LibraryQuery::Tracks`]
    /// does), by name. `album_artists` lists album artists instead (a track
    /// without one counts under its artist).
    Artists {
        search: Option<String>,
        #[serde(default)]
        album_artists: bool,
        #[serde(default)]
        filter: TrackFilter,
        offset: u32,
        limit: u32,
    },
    /// Folders holding tracks whose own name (not the folders above it)
    /// matches `search`, by path.
    Folders {
        search: Option<String>,
        offset: u32,
        limit: u32,
    },
    /// Tracks that counted as played (the scrobble rule), newest first; a
    /// track played twice is listed twice.
    RecentlyPlayed {
        offset: u32,
        limit: u32,
    },
    /// Albums with a track that counted as played, the latest play first.
    /// (Recently added albums: [`LibraryQuery::Albums`] sorted by `added`, descending.)
    RecentlyPlayedAlbums {
        offset: u32,
        limit: u32,
    },
    /// Index of an album's first track in [`LibraryQuery::Tracks`].
    AlbumPosition {
        search: Option<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        album: String,
        #[serde(default)]
        filter: TrackFilter,
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
        #[serde(default)]
        filter: TrackFilter,
        start: u32,
    },
    PlayAlbum {
        album: String,
        #[serde(default)]
        sort: Vec<SortRule>,
        /// Index of the track to start at; the first if left out.
        #[serde(default)]
        #[ts(optional)]
        start: Option<u32>,
        /// The album's tracks it lets through, as its card counts them.
        #[serde(default)]
        filter: TrackFilter,
    },
    /// Makes the tracks in a folder and the folders below it the context,
    /// by path, and plays from `start`.
    PlayFolder {
        path: String,
        start: u32,
    },
    /// The tracks of these albums, album after album in the order given,
    /// e.g. to queue them.
    AlbumTracks {
        albums: Vec<String>,
        #[serde(default)]
        sort: Vec<SortRule>,
        #[serde(default)]
        filter: TrackFilter,
    },
    AddRoot {
        path: String,
    },
    RemoveRoot {
        path: String,
    },
    Rescan,
    /// Marks or unmarks the library tracks at `paths` as favourites. Safe to repeat.
    SetFavourite {
        paths: Vec<String>,
        favourite: bool,
    },
    /// Rates the library tracks at `paths` in half stars (1 to 10), or clears
    /// their rating with `None`. Safe to repeat.
    SetRating {
        paths: Vec<String>,
        rating: Option<u8>,
    },
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
    /// The lyrics of the library track at `path`: embedded in the file, else
    /// a sidecar `.lrc` or `.txt`, else LRCLIB when that setting is on
    /// (fetched once, then cached). Synced beats plain within each source.
    Lyrics {
        path: String,
    },
    /// Writes the lyrics [`LibraryQuery::Lyrics`] finds as `<track>.lrc`
    /// next to the file, every time moved by `offset_ms` (positive shows
    /// lines later). Never touches the audio file.
    SaveLyrics {
        path: String,
        #[serde(default)]
        offset_ms: i32,
    },
    /// Saved playlists, by name.
    Playlists,
    /// A playlist's tracks in order. Entries whose files the library no
    /// longer has are left out (and come back with them).
    PlaylistTracks {
        id: String,
    },
    /// Makes the playlist the context and plays from `start`.
    PlayPlaylist {
        id: String,
        start: u32,
    },
    /// Makes a playlist with these tracks. `id` is the client's (e.g. a UUID),
    /// so a repeat does nothing.
    CreatePlaylist {
        id: String,
        name: String,
        #[serde(default)]
        paths: Vec<String>,
    },
    RenamePlaylist {
        id: String,
        name: String,
    },
    DeletePlaylist {
        id: String,
    },
    /// Appends tracks. `op` is the client's id for this addition, so a
    /// repeat adds nothing; the entries' ids are `<op>/<index>`.
    AddToPlaylist {
        id: String,
        op: String,
        paths: Vec<String>,
    },
    RemoveFromPlaylist {
        id: String,
        items: Vec<String>,
    },
    /// Moves an entry before another (`None`: to the end).
    MovePlaylistItem {
        id: String,
        item: String,
        before: Option<String>,
    },
    /// What this device's Downloaded group holds.
    Downloaded,
    /// Adds to or removes from the Downloaded group: linked engines' tracks
    /// it takes in are downloaded here, and copies no member needs any more
    /// deleted. Safe to repeat.
    SetDownloaded {
        member: Downloadable,
        downloaded: bool,
    },
    /// Where downloads are saved (a library folder; Music/Motif by default).
    SetDownloadFolder {
        path: String,
    },
}

/// Something in this device's Downloaded group (#245). The group is this
/// device's own: it isn't synced.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export)]
pub enum Downloadable {
    /// Following its edits.
    Playlist { id: String },
    /// By album key, as [`LibraryQuery::PlayAlbum`] names it.
    Album { album: String },
    /// Every album of an album artist (or artist, for tracks without one),
    /// later ones too.
    Artist { name: String },
    /// One library track, by path.
    Track { path: String },
}

/// How downloading the Downloaded group is going.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DownloadStatus {
    /// Where downloads are saved.
    pub folder: String,
    /// Tracks still to download.
    pub waiting: u32,
    /// The one downloading now.
    #[serde(default)]
    pub fetching: Option<Fetching>,
    /// Why the last one failed (the disk is full, its engine is unreachable).
    pub error: Option<String>,
}

/// The track downloading now and how far it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Fetching {
    /// Its catalog row's path, `remote:<engine>:<id>`.
    pub source: String,
    /// How much of it is here, in percent.
    pub percent: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum QueryResult {
    Status {
        status: LibraryStatus,
    },
    Downloaded {
        members: Vec<Downloadable>,
    },
    Tracks {
        total: u32,
        offset: u32,
        tracks: Vec<LibraryTrack>,
    },
    Played {
        total: u32,
        offset: u32,
        plays: Vec<PlayedTrack>,
    },
    Groups {
        groups: Vec<TrackGroup>,
    },
    Albums {
        total: u32,
        offset: u32,
        albums: Vec<LibraryAlbum>,
    },
    Artists {
        total: u32,
        offset: u32,
        artists: Vec<LibraryArtist>,
    },
    Folders {
        total: u32,
        offset: u32,
        folders: Vec<LibraryFolder>,
    },
    Position {
        index: Option<u32>,
    },
    /// Loudness (0–255, the loudest point 255) at evenly spaced points
    /// along the track.
    Waveform {
        levels: Vec<u8>,
    },
    /// `None` when no source has lyrics for the track.
    Lyrics {
        lyrics: Option<Lyrics>,
    },
    Playlists {
        playlists: Vec<Playlist>,
    },
    PlaylistTracks {
        entries: Vec<PlaylistEntry>,
    },
    Done,
}

/// A saved playlist, summed up like a [`LibraryAlbum`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub tracks: u32,
    #[ts(type = "number")]
    pub duration_ms: u64,
    /// The first track's artwork.
    pub art: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlaylistEntry {
    /// Names this entry for removing and moving; a track may be in a playlist twice.
    pub item: String,
    pub track: LibraryTrack,
}

/// A track's lyrics. Synced lyrics have a time on every line, in order;
/// plain lyrics have none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Lyrics {
    pub source: LyricsSource,
    pub lines: Vec<LyricsLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LyricsLine {
    /// When the line starts, from the start of the track.
    #[ts(type = "number | null")]
    pub time_ms: Option<u64>,
    pub text: String,
}

/// Where lyrics came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LyricsSource {
    /// Tags in the audio file.
    Embedded,
    /// A `.lrc` or `.txt` file next to it.
    Sidecar,
    Lrclib,
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
    /// Bumped whenever a playlist changes.
    #[ts(type = "number")]
    pub playlists: u64,
    /// Bumped whenever a track counts as played (Recently Played changes).
    #[ts(type = "number")]
    pub plays: u64,
    /// Bumped whenever a favourite mark or rating changes.
    #[ts(type = "number")]
    pub ratings: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub downloads: DownloadStatus,
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
    /// This release's year; see `original_year`.
    pub year: Option<u32>,
    /// When the music first came out, if the file says. Lists show this, falling back to `year`.
    pub original_year: Option<u32>,
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
    pub favourite: bool,
    /// Half stars, 1 to 10; none if unrated.
    pub rating: Option<u8>,
    /// The linked engines holding this audio, by key, this engine's own for
    /// a file here; empty until the engine knows its key. Clients show
    /// whether each is reachable from `NetState`.
    pub devices: Vec<String>,
}

/// A library track and when it was played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PlayedTrack {
    pub track: LibraryTrack,
    /// When the listen started, Unix seconds.
    #[ts(type = "number")]
    pub played_at: u64,
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
pub struct LibraryArtist {
    pub name: String,
    pub albums: u32,
    pub tracks: u32,
    /// Artwork of one of their albums.
    pub art: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LibraryFolder {
    /// As used by [`LibraryQuery::PlayFolder`].
    pub path: String,
    /// Tracks directly in the folder.
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

/// What a client may do. Every client may read the player state and the
/// library; each scope allows changing something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Scope {
    /// Transport, volume, repeat, and choosing what plays.
    Playback,
    /// Adding, moving and removing queue entries.
    Queue,
    /// Library folders, rescans, watching, saving lyrics, editing playlists.
    Library,
    /// Engine and integration settings (decoders, output, ReplayGain…).
    Settings,
    /// Pairing new devices and managing paired ones.
    Pairing,
    /// Getting this engine's library and synced data: a linked engine
    /// ([`PairedDevice::public_key`]).
    Sync,
}

/// The sets of [`Scope`]s offered when approving a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Preset {
    Full,
    ControlPlayback,
    ViewOnly,
}

impl Preset {
    pub fn scopes(self) -> Vec<Scope> {
        match self {
            Preset::Full => vec![
                Scope::Playback,
                Scope::Queue,
                Scope::Library,
                Scope::Settings,
                Scope::Pairing,
                Scope::Sync,
            ],
            Preset::ControlPlayback => vec![Scope::Playback, Scope::Queue],
            Preset::ViewOnly => vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum PairingCommand {
    /// Makes a one-time code for a new device, shown in
    /// [`PairingState::code`]. Replaces any earlier code.
    Start,
    /// Withdraws the current code.
    Cancel,
    /// Lets a waiting device ([`PendingDevice::id`]) in with these scopes.
    Approve {
        request: u32,
        preset: Preset,
    },
    Deny {
        request: u32,
    },
    /// Lets devices in without approval, with this preset; `None` (the
    /// default) asks every time.
    SetAutoApprove {
        preset: Option<Preset>,
    },
    Rename {
        device: String,
        name: String,
    },
    SetPreset {
        device: String,
        preset: Preset,
    },
    /// Forgets the device and closes its connections.
    Revoke {
        device: String,
    },
    /// Closes one connection ([`Connection::id`]); the device stays paired.
    Disconnect {
        connection: u32,
    },
    /// The LAN switch ([`LanState::allowed`]). Off closes the LAN to every
    /// device without revoking any; starting a pairing turns it on.
    SetLan {
        allowed: bool,
    },
    /// Turns the MPD server ([`MpdState`]) on or off.
    SetMpd {
        enabled: bool,
    },
    /// The password MPD clients send to get control; empty removes it,
    /// leaving them read-only.
    SetMpdPassword {
        password: String,
    },
    /// The internet switch ([`NetState::internet`]).
    SetInternet {
        allowed: bool,
    },
    /// Your own relay ([`NetState::own_relay`]); `None` or empty uses the
    /// public ones.
    SetRelay {
        url: Option<String>,
    },
    /// One of [`NetState::quality`].
    SetQuality {
        of: QualityFor,
        quality: Quality,
    },
    /// What this engine is called ([`PairingState::name`]).
    SetName {
        name: String,
    },
    /// The sending device's own engine key (ed25519, hex; the desktop app's
    /// or the phone's), linking the two engines: pairings from before links
    /// send it on connecting. Needs [`Scope::Sync`]; safe to repeat.
    SetEngineKey {
        key: String,
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairingState {
    /// The engine's public key (ed25519, hex), its identity.
    pub engine_key: String,
    /// What other devices call it: its computer's or phone's name until
    /// renamed.
    pub name: String,
    pub code: Option<PairingCode>,
    pub auto_approve: Option<Preset>,
    /// Devices that redeemed a code and wait for approval.
    pub pending: Vec<PendingDevice>,
    pub devices: Vec<PairedDevice>,
    pub connected: Vec<Connection>,
    /// Newest last, capped.
    pub log: Vec<PairingLogEntry>,
    pub lan: LanState,
    pub mpd: MpdState,
    pub net: NetState,
}

/// How linked engines reach each other (iroh, end-to-end encrypted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct NetState {
    /// The switch, kept across restarts: off, linked engines meet only on
    /// the LAN, directly.
    pub internet: bool,
    /// A relay URL of your own (`https://relay.example.com`); `None` uses
    /// the public iroh relays, which are slow (~4 Mbit/s).
    pub own_relay: Option<String>,
    /// Each linked device ([`PairedDevice::id`]) and how it is reached now.
    pub links: Vec<LinkState>,
    /// What linked engines send this one, kept across restarts. Streams on
    /// the LAN always get the original.
    pub quality: Qualities,
    /// Whether this device's network costs by the byte (Android says so;
    /// elsewhere never): streams then use [`Qualities::metered`].
    pub metered: bool,
}

impl Default for NetState {
    fn default() -> Self {
        Self {
            internet: true,
            own_relay: None,
            links: Vec::new(),
            quality: Qualities::default(),
            metered: false,
        }
    }
}

/// What a linked engine sends of a track: the file, or Opus at a bitrate.
/// Opus goes only for lossless files and those above its bitrate: a lossy
/// file at or below it comes as it is, as encoding it again would lose more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Quality {
    Original,
    Opus256,
    Opus192,
    Opus128,
    Opus96,
}

impl Quality {
    /// The Opus bitrate, or `None` for the original.
    pub fn kbps(self) -> Option<u32> {
        match self {
            Quality::Original => None,
            Quality::Opus256 => Some(256),
            Quality::Opus192 => Some(192),
            Quality::Opus128 => Some(128),
            Quality::Opus96 => Some(96),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Qualities {
    /// Streams off the LAN.
    pub stream: Quality,
    /// Streams off the LAN on a metered network.
    pub metered: Quality,
    /// Downloaded copies.
    pub download: Quality,
}

impl Default for Qualities {
    fn default() -> Self {
        Self {
            stream: Quality::Opus256,
            metered: Quality::Opus128,
            download: Quality::Opus256,
        }
    }
}

/// A track streaming from a linked engine: which one, what it sends and how
/// it gets here, which can change mid-track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Streamed {
    /// The linked engine's name.
    pub from: String,
    /// The original, or the Opus it encodes the file to.
    pub sent: Quality,
    pub route: Route,
}

/// How a connection to a linked engine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Route {
    /// Directly, on the same network.
    Lan,
    /// Directly, over the internet.
    Internet,
    /// Through a relay.
    Relayed,
}

/// Which of [`Qualities`] a [`PairingCommand::SetQuality`] sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum QualityFor {
    Stream,
    Metered,
    Download,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkState {
    pub device: String,
    pub reach: Reach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Reach {
    Direct,
    /// Through a relay, which sees only encrypted traffic.
    Relayed,
    Unreachable,
}

/// The MPD server for third-party clients (ncmpcpp, M.A.L.P.…): off by
/// default; on the LAN while [`LanState::allowed`], else this machine only.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MpdState {
    pub enabled: bool,
    /// Clients that send the password may control playback; without one
    /// set, every client is read-only.
    pub password_set: bool,
    pub port: u16,
    /// Enabled and listening (false if the port is taken).
    pub listening: bool,
}

/// Whether other machines can reach this engine.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LanState {
    /// The switch, kept across restarts.
    pub allowed: bool,
    /// Listening on every interface and advertised over mDNS: allowed, and
    /// a code is live, a device waits for approval or one is paired.
    pub open: bool,
    /// This machine's LAN addresses with the port (`192.168.1.5:7700`), the
    /// likeliest first. Empty while closed.
    pub addresses: Vec<String>,
    /// While open and a code is live: a URL per address
    /// (`http://<address>/pair?code=<code>&id=<engine key>`) for a QR code.
    pub pair_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairingCode {
    pub code: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PendingDevice {
    pub id: u32,
    pub name: String,
    /// The device's ed25519 public key (hex), if it has one (Motif apps).
    pub public_key: Option<String>,
    pub address: String,
    #[ts(type = "number")]
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    /// Its engine's key, if it has one (Motif apps); with [`Scope::Sync`]
    /// the engines are linked. A linked engine this one paired with has no
    /// token here.
    pub public_key: Option<String>,
    pub scopes: Vec<Scope>,
    #[ts(type = "number")]
    pub paired_at_ms: u64,
    #[ts(type = "number | null")]
    pub last_seen_ms: Option<u64>,
}

/// A client connected now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Connection {
    pub id: u32,
    /// The paired device, or `None` for this machine (loopback).
    pub device: Option<String>,
    pub address: String,
    #[ts(type = "number")]
    pub connected_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairingLogEntry {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub action: PairingAction,
    /// The device's name after the action.
    pub device: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PairingAction {
    /// Let in without approval.
    Paired,
    Approved,
    Denied,
    Revoked,
    Renamed,
    PresetChanged,
    /// Its engine became linked (sent its key, or this one paired with it).
    Linked,
}

/// Body of `POST /pair`, sent by a new device with the code it was shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PairRequest {
    pub code: String,
    pub name: String,
    /// The device's own engine key (ed25519, hex), if it has one (Motif
    /// apps; browsers have none).
    #[serde(default)]
    pub public_key: Option<String>,
}

/// The answer to [`PairRequest`] once the device is let in. `token` goes in
/// `Authorization: Bearer <token>` or a `token` URL parameter (for `/ws`,
/// `/art` and `/images`); the engine keeps only its hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Paired {
    pub device: String,
    pub token: String,
    pub engine_key: String,
    pub engine_name: String,
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
    fn playlist_wire_format() {
        let query: LibraryQuery = serde_json::from_str(
            r#"{"type":"move_playlist_item","id":"p1","item":"a1/0","before":null}"#,
        )
        .unwrap();
        assert_eq!(
            query,
            LibraryQuery::MovePlaylistItem {
                id: "p1".into(),
                item: "a1/0".into(),
                before: None
            }
        );
        let create: LibraryQuery =
            serde_json::from_str(r#"{"type":"create_playlist","id":"p1","name":"Road"}"#).unwrap();
        assert_eq!(
            create,
            LibraryQuery::CreatePlaylist {
                id: "p1".into(),
                name: "Road".into(),
                paths: vec![]
            },
            "an empty playlist needs no paths"
        );
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
