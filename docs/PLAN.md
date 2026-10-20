# Implementation plan

A music player for collectors and audio enthusiasts: plays anything, looks great,
runs on desktop and Android, remote-controllable, and needs no central server.

## Decisions

| Topic | Decision | Consequence |
|---|---|---|
| Licence | **GPL-3.0-or-later** | Free to use GPL FFmpeg builds, libopenmpt, game-music-emu, FluidSynth, soxr, etc. |
| Mobile | **Android only** for now; iOS deferred | No App Store constraints; iOS is a later port of the same UI |
| Distribution | GitHub releases, F-Droid, Flathub/AUR; **no Play Store** | Streaming sources (YouTube etc.) ship in every build |
| UI | **Svelte 5 + Vite** (plain SPA, no SvelteKit) | The same static bundle is loaded by Tauri and served by the engine to browsers |
| Shell | **Tauri 2** | One Rust core + one web UI for Linux/Windows/macOS/Android |
| Scope | All "missed features" below are in scope | Grouped into phase 9+ |

## Architecture

Everything is built around a **headless engine**. Every UI is a client of the
same protocol, including the desktop app's own window.

```
┌──────────────────────────── engine (Rust) ────────────────────────────┐
│ decode → DSP → output │ library DB │ queue/state │ p2p net │ plugins  │
└──────────────▲──────────────────────▲─────────────────────▲───────────┘
               │  WebSocket + JSON, types generated from Rust (ts-rs)
   desktop window (Tauri)    web UI (served by engine)    Android app / other devices
```

Consequences:
- Remote control is the normal code path, not an add-on.
- The web interface is the same frontend build the desktop window loads.
- A jam guest is just a client with a restricted permission set.

### Repository layout

```
crates/
  protocol/      wire types (Command, Event, PlayerState…), exported to TypeScript
  engine/        playback state machine, queue, audio pipeline
  server/        HTTP + WebSocket server, serves the web UI
  library/       (phase 1) scanning, tags, SQLite, artwork, smart playlists
  net/           (phase 6) iroh transport, pairing, discovery, sync, jam
  sources/       (phase 8) source plugins: local, YouTube, radio, Subsonic…
  acquire/       (phase 11) wanted list, Soulseek (slskd) search/download, post-processing
  integrations/  (phase 3) scrobbling, Discord, lyrics, OS media controls
apps/
  ui/            Svelte app (desktop, Android, web)
  desktop/       Tauri shell (+ Android native plugins under gen/android later)
  headless/      engine daemon for NAS / Raspberry Pi / web-only use
```

### Stack

| Concern | Choice |
|---|---|
| Decoding | Symphonia first, FFmpeg fallback ("plays anything") |
| Exotic formats | libopenmpt (trackers), game-music-emu (SPC/NSF/VGM…), FluidSynth (MIDI + soundfonts) |
| Output | cpal + exclusive/bit-perfect paths: WASAPI exclusive, ALSA `hw:`, CoreAudio hog mode, Android AAudio |
| Resampling | libsoxr (rubato as pure-Rust fallback) |
| Tags | lofty |
| Library DB | SQLite (sqlx) + FTS5; `notify` for folder watching |
| P2P | iroh: QUIC, hole punching, mDNS on LAN, relay fallback (self-hostable) |
| Sync | Automerge CRDT for metadata; BLAKE3 content-addressed files via iroh-blobs |
| Shared types | ts-rs → `apps/ui/src/lib/protocol/` (CI fails if stale) |

## Feature design

**Plays any audio file.** Probe the format, try Symphonia, fall back to FFmpeg.
Support gapless playback, CUE sheets (external and embedded), and ReplayGain /
EBU R128 (scanned in the background).

**Looks great + customisable layout.** The look is dense and sharp (Tauon /
Poweramp rather than soft cards). The window has fixed chrome (the left bar
for choosing what to browse, and the player bar) around a **music area**
arranged by **KDL layout files** ([LAYOUT.md](LAYOUT.md)): track lists (table
or album-grouped), cover gallery, artwork, track info, queue, in splits, like
foobar2000's or Tauon's view modes. Built-in layouts sit on toolbar buttons;
users install more by dropping a `.kdl` file onto the window. Appearance
(theme, accent, density, bar vs floating player) is a separate client-side
setting. Layouts and settings never touch the engine, which stays
playback-only. The player bar expands into a full now-playing screen
(dragged up on phones).

**Remote control.** JSON over WebSocket with state subscriptions. Devices pair
with a QR code and get a scoped, revocable token. The engine also speaks the
**MPD protocol**, so existing MPD clients work.

**Android + gestures.** The phone can play locally or remote-control another
device. Default gestures: swipe to skip, swipe down to minimise, long-press to
queue, vertical drag for volume or seek. Every gesture can be remapped.
Background playback needs a native `MediaSessionService` foreground service.
That is the biggest technical risk, so it gets a spike in phase 0.

**No central server.** LAN uses mDNS and direct connections. Over the internet,
iroh hole-punches and falls back to a relay that only sees encrypted traffic.
Public relays are the default and users can point at their own. Nothing needs
an account.

**Jam + listen-along (no login).** The host shares a ticket as a link or QR
code. Guests join from the app or from a browser; browser guests connect
directly on LAN or through the relay, since iroh runs in WASM.
- *Jam:* one shared queue with permissions (add / vote / skip / host approval).
  Audio plays on the host.
- *Listen-along:* every guest plays audio locally. The host broadcasts
  `(track, position, wall-clock)` and clocks align with an NTP-style offset
  estimate. Guests get audio either as an Opus stream from the host (local
  files) or by resolving the stream themselves (streaming sources). Drift is
  corrected with small rate adjustments, and hard seeks happen only past a
  threshold.

**Sync between devices.** Playlists, ratings, play counts and history merge
through a CRDT. Files sync selectively (e.g. "these playlists + 2 GB of most
played") and can be transcoded to Opus for the phone. Offline scrobbles from
every device merge into one queue.

**Streaming.** Implemented as source plugins: YouTube on desktop via a yt-dlp
sidecar, on Android via rustypipe or by asking a paired desktop to resolve
streams. The same plugin interface covers SoundCloud, Bandcamp purchases,
internet radio and Subsonic/Jellyfin servers.

**Soulseek acquisition.** Automatically finds and downloads music from
Soulseek into the main library. This is permanent *acquisition* and is
separate from the per-device "Downloaded" group, which only controls offline
copies on a device.
- *Client:* the engine talks to **slskd** through its REST API. slskd is a
  mature Soulseek daemon that can be bundled as a sidecar or run separately
  (Docker/NAS). This sits behind an `Acquirer` trait so a native Rust Soulseek
  client can replace it later. Android doesn't run slskd: it adds wants and
  the paired desktop/headless engine does the downloading.
- *Wanted list:* tracks, albums or whole artists. Wants can come from:
  - manual "want this" on any search result, artist or album page;
  - library gaps (missing albums from an artist's MusicBrainz discography,
    incomplete albums);
  - a streamed track (YouTube etc.), as "get a lossless copy";
  - Last.fm/ListenBrainz loved tracks, and imported playlists.
- *Search + scoring:* searches run on a schedule with rate limiting, since the
  network bans aggressive searchers. Each candidate gets a score from:
  - format and quality preferences (default FLAC, then 320 kbps MP3);
  - for albums, whether the folder's track count and durations match the
    MusicBrainz release;
  - the peer's free slots, queue length and upload speed;
  - blocklists of users and filename patterns.
  
  Albums download as whole folders. Results below a threshold go to a
  review queue instead of downloading.
- *Automation levels* (per want, with a global default): search only / ask
  before downloading / fully automatic. Failed or stalled transfers retry
  with the next-best candidate.
- *Post-processing:* decode-check every file and run the fake-lossless
  detector (reject or flag). Tag from MusicBrainz, rename and move into the
  library with a naming template, then rescan. Notify the user, and play
  counts or ratings from a matching streamed track carry over.
- *Sharing:* the Soulseek network expects sharing and bans leechers. Shared
  folders (default: the library, read-only), upload slots and speed limits
  are all settings. A listening port is needed, opened via UPnP/NAT-PMP or
  manual port forwarding.
- *Account:* Soulseek requires a username/password on its own server. This is
  a third-party network like YouTube, so it doesn't conflict with "no central
  server" for the app itself. Credentials go in the OS keyring.
- Every behaviour above is a setting; the values given are defaults.

**Scrobbling.** Last.fm, ListenBrainz and Libre.fm, using the standard rule
(half the track or 4 minutes) and an offline queue.

**Discord.** Rich Presence over local IPC using the "Listening to" activity
type, with artwork URLs from the Cover Art Archive. Android shows presence
through a paired desktop.

**Lyrics.** Lookup order: embedded USLT/SYLT tags, then a sidecar `.lrc`, then
LRCLIB. Synced lyrics display karaoke-style with an adjustable offset, and can
be saved back to the file or a sidecar.

## Roadmap

| Phase | Goal | Contents |
|---|---|---|
| **0** | Foundations | Workspace, protocol crate + TS bindings, engine skeleton, WS server, Svelte shell, Tauri shell, headless daemon, CI. **Spike: Android background audio.** |
| **1** | Desktop player | Symphonia/FFmpeg decode, cpal output, gapless, queue, library scan + DB, OS media controls (MPRIS, SMTC, macOS Now Playing) |
| **2** | Looks great | User themes, album-art palettes, drag/resize editing of layouts, per-device layouts, waveform seekbar, visualiser, artwork. (The layout file format, views and now-playing bar/widget were pulled forward into phase 0.) |
| **3** | Cheap wins | Lyrics, scrobbling, Discord, ReplayGain |
| **4** | Remote control | Pairing + tokens, LAN exposure, MPD protocol |
| **5** | Android | Android build, MediaSession plugin, gestures, local + remote modes |
| **6** | P2P + sync | iroh transport, device pairing, CRDT metadata, selective file sync, transcoding |
| **7** | Jam / listen-along | Sessions, permissions, browser guests, clock sync |
| **8** | Streaming | Plugin host, YouTube, radio, Subsonic/Jellyfin, SoundCloud, Bandcamp |
| **9** | Audiophile | Exclusive output, sample-rate switching, DSD (DoP/native), crossfade, PEQ + AutoEQ, convolution, crossfeed, speed/pitch, A-B repeat, sleep timer |
| **10** | Collection management | Tag editor + MusicBrainz, classical metadata, editions, library health, fake-lossless detection, spectrogram, DR meter, smart playlists + query language, folder view, imports (foobar2000/MusicBee/iTunes/Last.fm) |
| **11** | Soulseek acquisition | slskd integration, wanted list, scheduled search + scoring, automation levels, post-processing (verify, MusicBrainz tag, file into library), sharing settings, gap detection from discographies |
| **12** | Sources & outputs | SMB/WebDAV/SFTP folders, Subsonic API server, podcasts, audiobooks, Chromecast, AirPlay, UPnP/DLNA, Snapcast multi-room |
| **13** | Discovery & stats | bliss-rs similarity radio, ListenBrainz similar artists, local year-in-review |
| **14** | Platform & power users | Android Auto, widgets, Wear OS, media keys, command palette, WASM plugin API, accessibility, i18n, library backup/export |

## Phase 0 checklist

- [x] Licence, workspace, repository layout
- [x] `protocol` crate with ts-rs bindings
- [x] `engine` actor: queue + transport state machine (no audio yet)
- [x] `server`: `/ws` endpoint + static UI hosting
- [x] `headless` daemon
- [x] Svelte UI: connection, transport, queue, theme tokens
- [x] Tauri desktop shell embedding engine + server
- [x] CI (fmt, clippy, tests, binding freshness, UI check/build)
- [x] Music-area layouts (KDL) with built-in presets and drag-and-drop install; widgets: track list (table/grouped, resizable columns), album grid, artwork, track info, queue; settings (theme, accent, density, bar/floating player); expanding now-playing sheet
- [ ] Android background-audio spike (needs Android SDK/NDK)
