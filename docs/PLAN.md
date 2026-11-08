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
| Decoding | Built-in decoder plugins chosen per file: Symphonia, libopus, FFmpeg as an external program for the rest ("plays anything") |
| Exotic formats | libopenmpt (trackers), game-music-emu (SPC/NSF/VGM…), FluidSynth (MIDI + soundfonts) |
| Output | cpal + exclusive/bit-perfect paths: WASAPI exclusive, ALSA `hw:`, CoreAudio hog mode, Android AAudio |
| Resampling | libsoxr (rubato as pure-Rust fallback) |
| Tags | lofty |
| Library DB | SQLite (sqlx) + FTS5; `notify` for folder watching |
| P2P | iroh: QUIC, hole punching, mDNS on LAN, relay fallback (self-hostable) |
| Sync | Automerge CRDT for metadata; BLAKE3 content-addressed files via iroh-blobs |
| Shared types | ts-rs → `apps/ui/src/lib/protocol/` (CI fails if stale) |

## Feature design

**Plays any audio file.** Decoders are built-in plugins behind one interface;
for each file the engine picks the best one that can open it and falls back to
the next on failure (details in the phase 1 plan).
Support gapless playback, CUE sheets (external and embedded), and ReplayGain /
EBU R128 (scanned in the background).

**Looks great + customisable layout.** The look is dense and sharp (Tauon /
Poweramp rather than soft cards). The window has fixed chrome (the left bar
for choosing what to browse, and the player bar) around a **music area**
arranged by **KDL layout files** ([LAYOUT.md](LAYOUT.md)): track lists (table
or album-grouped), cover gallery, artwork, track info, queue, in splits, like
foobar2000's or Tauon's view modes. Built-in layouts sit on toolbar buttons;
users install more by dropping a `.kdl` file onto the window. Appearance
(theme, user [theme files](THEMES.md), accent, density, bar vs floating player) is a separate client-side
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
| **4** | Remote control | Pairing (device keys + scoped tokens, the one pairing system phase 6 reuses), LAN exposure + mDNS, pairing UI, MPD protocol |
| **5** | Android | Android build, MediaSession plugin, gestures, local + remote modes |
| **6** | P2P + sync | iroh transport for devices paired in phase 4 (encrypted, over the internet), CRDT metadata, selective file sync, transcoding |
| **7** | Jam / listen-along | Sessions, permissions, browser guests, clock sync |
| **8** | Streaming | Plugin host, YouTube, radio, Subsonic/Jellyfin, SoundCloud, Bandcamp |
| **9** | Audiophile | Exclusive output, sample-rate switching, DSD (DoP/native), crossfade, PEQ + AutoEQ, convolution, crossfeed, speed/pitch, A-B repeat, sleep timer |
| **10** | Collection management | Tag editor + MusicBrainz, classical metadata, editions, library health, fake-lossless detection, spectrogram, DR meter, smart playlists + query language, folder view, imports (foobar2000/MusicBee/iTunes/Last.fm) |
| **11** | Soulseek acquisition | slskd integration, wanted list, scheduled search + scoring, automation levels, post-processing (verify, MusicBrainz tag, file into library), sharing settings, gap detection from discographies |
| **12** | Sources & outputs | SMB/WebDAV/SFTP folders, Subsonic API server, podcasts, audiobooks, Chromecast, AirPlay, UPnP/DLNA, Snapcast multi-room |
| **13** | Discovery & stats | bliss-rs similarity radio, ListenBrainz similar artists, local year-in-review |
| **14** | Platform & power users | Android Auto, widgets, Wear OS, media keys, command palette, WASM plugin API, accessibility, i18n, library backup/export |

## Phase 4 decisions (2026-11-03)

- **One pairing system.** Phase 4 builds it around device keys (ed25519, iroh's
  key type) and a paired-devices list with scoped, revocable tokens. Phase 6
  adds iroh as a second transport for the same devices; it does not pair again.
- **LAN in phase 4 is plain HTTP/WebSocket plus a token.** Someone sniffing
  the same network could read a token; the encrypted path is iroh (phase 6).
  Loopback stays trusted without a token.
- **Discovery:** the QR code carries the address; the engine also advertises
  `_motif._tcp` (and `_mpd._tcp` when MPD is on) over mDNS.
- **Permissions** are stored as scopes (playback, queue, library, settings,
  pairing) so phase 7 jam guests fit; the UI offers Full / Control playback /
  View only.
- **Pairing flow:** the host shows a QR (address + one-time code); the new
  device opens it and the host approves it (approval is on by default and
  can be switched off). Headless prints the QR in its terminal, and any Full
  device can start or approve pairing.
- **Exposure:** loopback only until pairing starts, which opens the LAN. It
  stays open while any LAN device is paired; a switch closes it without
  revoking anyone.
- **MPD** is off by default, loopback only unless LAN is also allowed; a
  password gives control, without one it is read-only.

## Phase 5 decisions (2026-11-04)

Tickets #117–#133 (milestone "Phase 5").
- **Storage:** All files access (`MANAGE_EXTERNAL_STORAGE`) with plain paths,
  so the scanner and sidecars work unchanged. Fine off the Play Store.
- **One engine at a time:** the app shows "This phone" or one paired engine
  and switches between them; local playback keeps going in the background.
  Engines and their tokens are client settings.
- **Adding an engine:** mDNS list + code, in-app QR scan, and pairing links
  that open the app.
- **The phone is not a pairable host in phase 5,** but "This phone" is an
  engine entry like any other so it can become one later.
- **Remote playback controls** (notification, lock screen) are opt-in, off
  by default. Volume keys control the remote engine in remote mode.
- **Gestures** (touch only, also on touchscreen desktops): swipe the bar or
  artwork to skip, long-press a row to queue, vertical drag on the artwork
  for volume. All remappable (Settings → Gestures: also seek, queue next or none).
- **Deferred:** signed release builds, F-Droid metadata and battery-killer
  testing on release builds.

## UI rework decisions (2026-11-08)

Phase 5 ends with a new navigation and look, designed from the user's phone
screens (layout only, not colours, sizes, fonts or icons). Clickable mock-up:
[docs/design/mockup/index.html](design/mockup/index.html). It is one design
language for phone and desktop, not a separate mobile UI.
- **Navigation:** Library home (All Songs, Albums, Playlists, Recently
  Played, Recently Added) → a category → an album or playlist page; Search
  and Settings are views too. On the phone a bottom bar (mini player,
  progress, Library / Search / Menu) replaces the drawer; Menu holds the
  connection and engine switcher, Settings, Changelog and Help. On desktop
  the left panel is the Library home and the player bar stays; views open in
  the music area with a back button.
- **Desktop layouts stay as they are** and keep arranging the library (All
  Songs) view. Phones show the new views instead, one widget per view (no
  stacked splits, which closes #157).
- **Collection views** (albums, playlists): cover grid, name list or list
  with covers; grid columns and title/artist visibility configurable.
  **Song views** (album, playlist, All Songs, recents): song list first,
  also list with covers and grid. View options are per device, per kind of
  view, and per playlist.
- **New data:** play history (Recently Played) and saved playlists are
  built now. Auto-radio suggestions after the queue wait for similarity
  radio (phase 13).
- **⋮ menus** hold view options, sort, Rescan and Add to launcher (an
  Android home-screen shortcut to that view; not on desktop).
- **Changelog** is a bundled CHANGELOG.md; **Help** opens the docs on GitHub.
- **Later in the phase:** swipe right = back (not in now playing), swipe
  left on a song = queue it (in the queue: remove it), tapping the cover in
  now playing cycles the album's images, swipe down closes now playing;
  screen changes animate with shared elements (cover, title) moving into
  place.

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
- [x] Android background-audio spike (results below)

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

Android (2026-11-06, #131): Sony Xperia (XQ-DC54, 60 Hz), Android 15,
Android System WebView 156, debug APK's WebView navigated to the dev server
over `adb reverse`, 50k tracks. Portrait (384×896 CSS px, phone layout):

| | Track list | Album grid | List + grid (stacked) |
|---|---|---|---|
| With the blurred panel | 60 | 60 | 12–33 |
| Scroll only | 60 | 60 | — |

Renderer memory stays near 300 MB for one widget, but the stacked list +
grid renders all 50,000 rows and every card (2.2–2.7 GB): the first run got
the renderer killed by Android's low-memory killer, taking the app and its
playback with it. Tickets: #157 (stacked splits), #158 (survive a renderer
death). Landscape (896×384, desktop layout) gave 60 everywhere except list
+ grid scroll-only at 46–51.

**The playback clock looped on phones.** Its tick read the clock state it
had just written, so the effect that starts it reran on its own write for
as long as `Date.now()` moved on between runs: on the phone Svelte's update
limit was hit every tick, with ~500 forced style and layout passes a
second. Phone Chrome 156 with the same page (the app's WebView can't reach
a test engine): playing cost 62% of a core with the sheet closed, 76% open,
87% on the lyrics view. Fixed: 7.1%, 7.3%, 8.6% (paused: 4.5%).

### Android background-audio spike (2026-10-24)

The generated Tauri Android project (minSdk 26, for AAudio) is on master. The
branch `spike/android-audio` adds what was tested on top: a foreground
`mediaPlayback` service with a media session and notification
(`PlaybackService.kt`), and a quiet 440 Hz cpal tone while the engine plays
(`apps/desktop/src/spike_tone.rs`). Sony Xperia XQ-DC54, Android 15, debug
build:

- The UI loads from the APK and talks to the in-process engine over its
  loopback WebSocket (the server's origin check accepts the Android WebView).
- Screen off, battery unplugged (simulated) and Doze forced into deep idle
  for 30 minutes: same process the whole time and the AAudio stream stayed
  `started`. Android froze other background apps (WhatsApp, Snapchat) during
  the run but not Motif. The tone was audible throughout and the
  notification showed.

Still to do in phase 5: bring the service back from the branch, running only
while something plays, with its buttons wired to the engine (a Tauri
plugin); apply the system bar insets: the player bar sits under
the navigation bar and the toolbar under the status bar. Not tested yet:
real (not forced) overnight Doze, Sony's STAMINA mode, other vendors'
battery killers, and release builds.

### Background playback (#119, 2026-11-04)

Without a foreground service, Android freezes Motif about 3 s after it
goes to the background on battery, even mid-track (Sony, Android 15,
`dumpsys battery unplug`; on USB power it was never frozen, which hides
the problem). `PlaybackService.kt` now runs as a `mediaPlayback` foreground
service while the engine plays or is paused, with a media session for the
notification, lock screen and headset. `MediaPlugin.kt` is a Tauri plugin
registered from the shell (`apps/desktop/src/media.rs`): Rust follows the
engine with `integrations::media_controls::follow` and calls `show`; the
buttons come back over a Rust `Channel` as plain `protocol::Command` JSON.
Kotlin-to-Rust calls for later Android work can use the same pattern.

- The notification needs no `POST_NOTIFICATIONS` permission: media-session
  notifications are exempt, so none is asked.
- Android 12+ refuses to start a foreground service from the background.
  Playback started from the UI is fine, and the service stays up while
  paused, so the media buttons never need to start it. Playback started by
  a remote client while Motif is in the background plays without the
  service (so it can be frozen) until Motif is next opened. Fine while the
  phone is not a pairable host.

### Remote playback controls (#126, 2026-11-06)

With Settings → Devices → Remote playback controls on, a remote engine's
playback takes over the phone's one media session (`Playback` in
`PlaybackService.kt` keeps the phone's and the remote's state apart and
shows the remote's while it's set). The UI's existing connection drives it:
`apps/ui/src/lib/remoteMedia.ts` sends the remote's track, state and
artwork URL to `MainActivity.setRemoteMedia` on every change, Kotlin
downloads the art, and the buttons come back as `protocol::Command` JSON
that the UI sends to the remote engine. Nothing goes through the phone's
engine. Disconnected, stopped or nothing queued sends `null`: the phone's
engine shows again and the service stops if that is idle too. Audio focus
stays with the phone's engine.

- The connection stays in the WebView. Wry's `onPause` calls
  `WebView.onPause()`, which doesn't pause JavaScript, and the foreground
  service keeps the process from being frozen, so the socket stays up.
  Moving it to the native side would mean a second client with its own
  pairing token; only worth it if the phone test shows the WebView dropping
  it.
- Phone test (Sony, Android 15, 2026-11-06, a muted headless engine on
  the LAN): track and art show in the notification and on the lock screen;
  play, pause, next, previous and seek from both reach the remote (checked
  against its state); stopping it, killing it (back within 4 s) or turning
  the setting Off hands the session back to the phone's engine, and
  reconnecting brings the remote back. The phone's engine had a paused
  queue, so its own notification showed then instead of none, as designed.
  With the setting Off, the phone's own playback and buttons work as before.
- Not checked yet: the connection over minutes in the background with the
  screen locked (the buttons were tried on the lock screen right after
  locking), and the hour-idle battery number.

### Background library work (#140, 2026-11-05)

Without the playback service, Android killed a backgrounded Motif for
"excessive cpu" mid-hash: `top -H` showed the `library-scan` thread at one
full core (nice -10). Now `MediaPlugin.kt` reports "hold" while the app is
out of sight (`onStop`) and `PlaybackService` isn't running, and the shell
calls `Library::hold_background`: hashing and thumbnails wait, scans still
run. Backgrounded 11 min on `dumpsys battery unplug` + `deviceidle
force-idle` mid-hash (3069 files left), the thread used no CPU and Motif
survived; opening it resumed hashing.

### Big library on the phone (#133, 2026-11-07)

Sony XQ-DC54, Android 15. A separate muted `motif-headless` (cross-built,
run from `/data/local/tmp` with its own data dir) scanned the real
collection in `/storage/emulated/0/Music` (8,181 tracks, through Android's
FUSE) plus 12,000 generated two-second FLACs (one 500 px `cover.jpg` per
album): 20,181 tracks. Desktop numbers are in phase 1, milestone 5.

| | Before | After |
|---|---|---|
| First scan (tags, no hashing) | 398 s (~20 ms a file) | not re-run (scan code unchanged) |
| Unchanged rescan, also at every start | 5.1 s, 0 files re-read | 5.3 s |
| Database (with WAL) | 17.2 MB after the scan, 19.2 MB hashed | 19.2 MB |
| Artwork cache | 894 MB (630 full-size copies, 710 bar thumbnails) | 6 MB (thumbnails only) |
| Engine RSS | 114 MB just after the scan (peak 245 MB while scanning), 26 MB hashing after a restart | 49 MB just after start-up |
| Waveforms | made on demand, ~1 KB a track in the DB | unchanged |

The real app's cache on the phone was 951 MB for 8,181 tracks (631 copies,
up to 36 MB each), with 3.1 GB free on the phone. **Fix:** embedded covers are
no longer copied. The art row points at the track, and the full-size cover is
read from its tags when asked for. Thumbnails are cached as before,
including the 128 px bar size made for every cover at scan. DB version 10
re-points old copies at a present track and deletes them. After the upgrade
on the phone, 40 of 40 full-size album covers were served from the tracks.
If the track a cover is read from is gone or has a new cover, it is read
from another present track that shows it. Limit (a `shortcut:` in
`scan.rs`): once every track with a cover is missing (an unplugged SD card),
only its thumbnails already made show. A copy whose tracks are all missing
at upgrade is kept.

**Rescan after every restart: ruled out.** A start-up scan of the unchanged
20k library re-reads nothing and takes 5 s. Full re-reads do happen once
after installing a build that raises the DB version (versions 7–9 each set
`mtime = -1` for every track). That costs about 20 ms a file on this phone,
roughly 3 min for the real collection, which is likely what was seen.

**Memory while browsing and playing** (master 7f32ffd debug build, the real
app with the 8,181-track collection; `ATTACH=127.0.0.1:7799 ADB_PID=<pid>
node scripts/soak.mjs mix 1800`, app-process RSS: Rust engine and Java,
not the WebView's renderer): 275 MB at the start, then a sawtooth between
about 145 MB and 1.9 GB for 30 minutes (50 rounds), ending at 170 MB. Nothing
accumulates. The peaks probably come from the `art` step, which asks for every album's
cover at once, at a new thumbnail size each round plus full size, so
hundreds of 3000 px covers are read from their tracks and decoded together.
Browsing asks only for the rows in view, so it doesn't do this, but a
smaller phone could kill the app at such a peak. A separate `art` run (10
min, 20 rounds, library re-hashing in the background) reached 1.3 GB, so
the covers do cause it. Serving 6 covers at once instead of 50 (`ART_SLOTS`)
made no difference (1.17 GB, 22 rounds), so it isn't the decodes at once.
Still open: where it goes (heap profile, or the allocator keeping freed
memory: RSS falls back only minutes after the requests stop).

The soak also showed that the library listed every Opus track as damaged
("unsupported feature: core (codec)"): its hash/waveform/loudness decoder
was Symphonia alone. Fixed in d46a987 (libopus for Opus, DB version 11
checks noted files again). Opus files with covers over 16 MB (#137) still
can't play: Symphonia's Ogg reader refuses packets over 16 MiB.

Not measured yet: hashing time for the whole library (the generated files
hash much faster than real ones) and a first scan with the new build.

## Phase 1 plan: desktop player

*Done 2026-10-26: all eight milestones below. CUE sheets and tracker, game
and MIDI formats were left for phase 10.*

Goal: point Motif at your music folders on Linux, Windows or macOS and it
plays them: gapless, every common format, real library browsing with
artwork, and the OS media keys and widgets work. Android keeps compiling
(the engine's audio code is cross-platform) but is tested in phase 5.

### Where things live

- **`crates/engine`**: the audio pipeline (decode → resample → volume →
  output), the decoder plugins and the queue. Still playback-only.
- **`crates/library`** (new): folder scanning, tags, identity keys, the
  SQLite database, artwork extraction, file watching. It knows nothing about
  playback; the engine knows nothing about it.
- **`crates/protocol` / `crates/server`**: library queries go over the same
  WebSocket; artwork is served over HTTP (`/art/<id>`) so the UI can use
  plain `<img>` tags and the browser cache. Library data is not a UI concern,
  so this fits the engine rule; layouts, themes and view state still stay out.
- **`apps/desktop` / `apps/headless`**: wire engine + library + server
  together, supply the engine's resolver (below), and own OS integration
  (media controls).

### Decoder plugins

All decoders implement one `Decoder` trait and are compiled in; nothing is
loaded at runtime.

- Each plugin declares what it can open (file signatures, extensions,
  codecs) and a quality rank per format.
- For each file the engine tries the best-ranked plugin that claims it and
  falls back to the next on failure (corrupt header, FFmpeg missing).
- The order is a user setting with defaults that just work.
- Each platform builds the plugins that suit it (cargo features); Android
  leaves out the FFmpeg process.

Phase 1 plugins:

| Plugin | Formats | Notes |
|---|---|---|
| Symphonia | FLAC, MP3, AAC/ALAC (MP4), Vorbis, WAV/AIFF/CAF, ADPCM | Pure Rust, sample-exact seeking |
| libopus | Opus | The most common format Symphonia lacks; exact seeking and gapless |
| FFmpeg (external program) | WavPack, APE, DSD (as PCM), WMA, TTA, anything else | Uses `ffmpeg` from the PATH or bundled with releases (an audio-only GPL build); seeking restarts the process; a crash only kills that process |

Later plugins (libopenmpt, game-music-emu, FluidSynth, native DSD) slot in the
same way.

### One library per device

Each device keeps its own library database, because only it knows which
files it has and where they are. On one computer the desktop app and the
headless engine share it: it lives in the user's data directory, the first
engine to start owns it, and the other connects to that engine as a remote
instead of starting its own. Ratings, play counts, playlists and history are
shared between devices by sync in phase 6, not by sharing the database.

The library database is the source of truth for what a device has. A file
that disappears is marked missing rather than forgotten, and a file that
won't decode is marked with the reason, so the library can later offer to
find or fetch them again (library health, phase 10). Removing a folder in
Motif is the only thing that deletes tracks. Library management happens in
Motif first; changes other programs make to the folders are picked up as
they happen when "pick up changes made outside Motif" is on (the default),
and by Rescan either way.

So a queue item can be played by whichever device has it, every track gets
**identity keys** when it is scanned:
- MusicBrainz recording/release IDs and ISRC from tags, when present;
- normalised artist, title, album and duration, as a fallback;
- a hash of the audio data only (tag edits don't change it), to recognise
  the exact same file. Hashing reads every file in full, so it runs in the
  background after the scan and never delays browsing or playback.

The engine resolves queue items through a **resolver** it is given rather
than knowing about the library: "what on this device can play this
reference?". The order is a local file on this device, then a synced or
downloaded copy (phase 6), then the original streaming source, then a match
on another source (phase 8). A track nothing can play is reported, not
skipped silently.

### Milestones

Each milestone ends with every check passing, a commit, and something you can
try.

1. **Track references.** *(done)* Replace `TrackInfo` in queue items with the
   source-independent reference: `source` + `id`, identity keys when known,
   plus title, artist, album and duration for display. The engine plays a
   reference through the resolver; for now the resolver only knows local
   paths. Protocol v4.
2. **It makes sound.** *(done)* The decoder trait and the Symphonia plugin. One
   decode thread feeds a lock-free ring buffer that the cpal output callback
   drains. Play, pause, seek and volume act on real audio; position comes
   from samples actually played, replacing the simulated clock. Volume is a
   smoothed software gain. Output runs in shared mode at the device's rate,
   and tracks at other rates go through rubato.
3. **Gapless.** *(done; AAC in MP4 keeps ~23 ms of encoder delay because Symphonia ignores the edit list)* Decode the next queue item ahead of time and splice it into
   the same buffer at the exact sample, honouring encoder delay and padding
   (LAME/iTunSMPB, Opus pre-skip). One resampler carries on across tracks
   that share a sample rate, so even resampled tracks join seamlessly; a rate
   change flushes it at the boundary. Test: two halves of one sine
   wave played back to back show no discontinuity.
4. **More decoders.** *(done; the order is not saved across restarts until milestone 6)* The libopus plugin, then the FFmpeg plugin. Choosing
   and falling back between plugins, the order setting, and a per-file
   reason when nothing can play it ("needs FFmpeg").
5. **Library scan.** *(done. Measured on an 8,267-track library on a USB
   hard drive through NTFS-3G: first scan 7 minutes, disk-bound at ~50 ms a
   file, and parallel reads didn't help; unchanged rescan 0.5 s; playback
   doesn't underrun while hashing.)* `library` walks the configured folders, reads tags with
   lofty, and stores tracks/albums/artists with their identity keys in SQLite
   (rusqlite with its bundled SQLite, FTS5 for search). Rescans are
   incremental by path, size and mtime; `notify` picks up changes while
   running; audio hashes fill in in the background. Embedded and folder
   artwork (`cover.jpg`, `folder.png`…) is extracted once into a cache with
   thumbnails. The resolver now matches references to library tracks by
   identity key. The library-owner rule for desktop + headless lands here.
   Targets: 50k tracks scanned in under 2 minutes on an SSD (hashing
   excluded), unchanged rescans in seconds.
6. **Library in the UI.** *(done)* The demo library goes (it stays as the
   `dev-library` stress-test generator). Track lists and the album grid
   fetch windows of rows from the server, with sorting, grouping and search
   done in SQL, so 50k tracks never cross the wire at once. Settings get a
   Library section: folders (native picker under Tauri, a path field in a
   browser), rescan, scan and hashing progress, decoder order. Album art
   replaces the placeholder gradients.
7. **OS media controls.** *(done; verified on Linux/MPRIS, Windows and macOS only compile in CI)* souvlaki in the desktop and headless apps: MPRIS
   on Linux, SMTC on Windows, Now Playing on macOS, so media keys, the
   lock screen and desktop widgets show and control Motif.
8. **Hardening.** *(done. Measured: no underruns while a full scan or
   background hashing reads the same USB drive as playback; engine CPU 0.1%
   of a core idle, 0.5% paused, 0.8% playing FLAC resampled to 48 kHz. Not
   tested: unplugging a real output device.)*
   - Unreadable, missing or corrupt files are skipped with a visible reason.
   - A lost output device (headphones unplugged) pauses rather than
     erroring, and an output device picker is added.
   - CI installs ALSA headers for cpal on Linux and runs decode and gapless
     tests against small generated fixture files.
   - Performance budgets: playback stays glitch-free while scanning and
     hashing, and idle CPU stays under 1%.

### Deferred on purpose

Bit-perfect/exclusive output, sample-rate switching, native DSD, crossfade
and EQ (phase 9); ReplayGain (phase 3); CUE sheets and tracker/game/MIDI
formats (late in phase 1 if time allows, otherwise phase 10); tag editing
(phase 10); libsoxr (phase 9, rubato until then); AcoustID fingerprints as
an identity key (phase 10); measuring background hashing cost on phones
(phase 5).

## Phase 2 notes

*Done 2026-10-30: everything in the roadmap row (user themes, album-art
palettes, layout editing, per-device layouts, waveform seek bar, visualiser,
artwork). Phase 3 is next.*

### Waveform seek bar (2026-10-29, #16)

The library decodes a track's waveform when it is first asked for (the
playing track and the next two), 1000 RMS levels of a byte each, and caches
them by audio hash in `library.db`; nothing runs over the whole library.
Timed over `/ws` against a release `motif-headless` on a Ryzen 5 3600:

| File | First request | Cached |
|---|---|---|
| 4 min FLAC, 16-bit/44.1 kHz stereo | 142 ms | 1 ms |
| 1 hour FLAC, same format | 1.86 s | 1 ms |
| 4 min MP3 192 kbps (also hashed in the same decode) | 210 ms | 1 ms |

An answer is about 3.9 KB of JSON. The full seek bar shows the plain line
until it arrives. Formats the library's Symphonia decode can't read (those
the engine plays with libopus or FFmpeg) keep the plain line.

### Visualiser (#17, 2026-10-29)

**Choice: the engine computes spectrum bands** (option 1 in #17). While at
least one client subscribes (`{"id":…,"bands":true}`), the output callback
copies what it plays, before the volume, into a second lock-free ring buffer
(wait-free, no allocation, dropped if full). Every 33 ms the engine task runs
a 2048-point FFT (`realfft`, already built for rubato) over the latest
samples and publishes 32 log-spaced bands, 40 Hz–16 kHz, as bytes (0 = 60 dB
down, 255 = full scale) in `Event::Bands`. The engine notices subscribers on
its existing 100 ms tick; without any, the tap is off and nothing extra runs.
The `visualiser` widget draws bars on a canvas as frames arrive, and
subscribes only while playing, on screen and in a visible window. MilkDrop
(raw PCM) is a possible later bonus.

Measured with a release `motif-headless` (volume 0) and WebKitGTK 2.52.6
MiniBrowser, X11, Ryzen 12 threads, RTX 3060, a 300×120 widget:

| | Cost |
|---|---|
| Engine CPU (tap, FFT, JSON, WebSocket) | +0.4% of a core (0.75% → 1.2% while playing) |
| Engine CPU, unsubscribed | unchanged; no frames sent after unsubscribing |
| Bandwidth | 30.3 frames/s × 116 B = 3.5 KiB/s |
| WebKit, CPU rendering (default on NVIDIA) | +5% of a core (2.0–2.5% → 6.6–7.5%): ~1.2% receiving frames, ~3.7% painting |
| WebKit, GPU rendering (desktop shell's SHM mode) | +9% (2.7% → 12%) |
| WebKit, paused | 0.1% |

Latency: bands describe audio as the callback hands it to the device, and
reach the client 0–33 ms later (half a tick on average; loopback WebSocket
under 1 ms), then wait up to a display frame. The device plays it about
32 ms after the callback (cpal timestamps, 512-frame callbacks through
PipeWire), so pictures land within roughly ±20 ms of the sound, on average
slightly early. That is inside what viewers notice (lip-sync guidance allows
~45 ms early, ~125 ms late). Remote clients would add their network delay.

Painting is the main cost and scales with the frame rate (each frame
recomposites the window); if that matters on slow machines, draw fewer
frames.
