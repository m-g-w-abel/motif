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
- *Search:* guests find songs without leaving Motif. The jam page searches
  everything the **host** can play: its local library plus its catalog
  sources (YouTube Music, and Spotify if the host has signed in through a
  Spotify source plugin). Results show where each track would play from.
  This needs no guest account and works for browser guests. Guests' own
  streaming libraries (e.g. their Spotify playlists) are deliberately not
  supported: Spotify's official API caps third-party apps at five users, and
  the alternatives break its terms.
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

Queue items are source-independent track references (source IDs plus title,
artist, album, duration and ISRC), resolved to something playable by the
device that plays them: a local copy first (collectors prefer their own
files), then the original source, then a match on another source. A track
nothing can play is reported, not silently dropped. This keeps jam and
listen-along working when devices have different sources available. Design
the reference format during phase 1 so the queue doesn't need reworking
when sources arrive.

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
- [x] Webview stress test on WebKitGTK (results below)
- [x] Android background-audio spike (results below; branch `spike/android-audio`)

### Webview stress test (2026-10-24)

`?dev-stress&dev-library=50000` scrolls every track list and album grid each
frame under a panel with `backdrop-filter` blur, a blurred artwork layer and a
64-bar canvas visualiser, and shows the frame rate (`window.stress` for
scripts). `?dev-stress=scroll` leaves the panel out. WebKitGTK 2.52.6, X11,
RTX 3060 (NVIDIA driver), 1280×800, 50k tracks:

| | Track list | Album grid | List + grid |
|---|---|---|---|
| Software rendering, before | 62 | 12 | 18 |
| Software rendering, after | 62 | 62 | 62 |
| GPU rendering (SHM) | 60 | 60 | 60 |

With the blurred panel on top, software rendering now gives 54–59 fps.

What was wrong, and the fixes:
- **WebKitGTK turns GPU rendering off on NVIDIA's driver.** Its GBM buffers
  fail there, so the hardware acceleration policy is forced to "never" and
  the CPU draws everything. The desktop shell restarts itself with
  `WEBKIT_FORCE_DMABUF_RENDERER=1 WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` when it
  finds the NVIDIA driver, unless the user set those variables. The
  surfaceless mode (`WEBKIT_DMABUF_RENDERER_DISABLE_GBM`) draws a blank window.
- **Blurred `box-shadow` is slow on CPU rendering.** Album cards had one, and
  the grid repaints on every scroll; a 1px border took the grid from 12 to
  62 fps. Keep shadows to popups that don't sit over scrolling content.
- **The playback clock ran every frame.** It changed `--fraction` (forcing
  layout) and `aria-valuenow` on every progress bar 60 times a second: 9% of
  a core while playing, 43% with the now-playing sheet open. The clock now
  ticks once per second, and bars advance a pixel at a time on a timer,
  moved with transforms. Playing now costs 1.4–2.2%.
- **Composited animations stall WebKit's GPU renderer.** A first version of
  the bars used the Web Animations API; with GPU rendering on, the phone
  layout then froze for ~1.4 s every 2 s. Plain timer updates don't.

Android is untested until it has a build.

### Android background-audio spike (2026-10-24)

Branch `spike/android-audio`: the generated Tauri Android project (minSdk 26,
for AAudio), a foreground `mediaPlayback` service with a media session and
notification, and a quiet 440 Hz cpal tone while the engine plays. Sony
Xperia XQ-DC54, Android 15, debug build:

- The UI loads from the APK and talks to the in-process engine over its
  loopback WebSocket (the server's origin check accepts the Android WebView).
- Screen off, battery unplugged (simulated) and Doze forced into deep idle
  for 30 minutes: same process the whole time and the AAudio stream stayed
  `started`. Android froze other background apps (WhatsApp, Snapchat) during
  the run but not Motif.

Still to do in phase 5: wire the media session's buttons to the engine (a
Tauri plugin), and apply the system bar insets: the player bar sits under
the navigation bar and the toolbar under the status bar. Not tested yet:
real (not forced) overnight Doze, Sony's STAMINA mode, other vendors'
battery killers, and release builds.

