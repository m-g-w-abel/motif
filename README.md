# Motif

A music player for collectors: it plays almost any audio file, runs on Linux,
Windows, macOS and Android, can be controlled from any other device, and needs
no account or central server.

Motif is built around a **headless Rust engine**. Every interface, including the
desktop app's own window, is a client of the same WebSocket protocol, so remote
control isn't an add-on: it's the normal code path.

```
┌──────────────────────────── engine (Rust) ────────────────────────────┐
│   decode → resample → output  │  library (SQLite)  │  queue / state   │
└──────────────▲──────────────────────▲─────────────────────▲───────────┘
               │  WebSocket + JSON, types generated from Rust (ts-rs)
   desktop window (Tauri)    web UI (served by engine)    Android / MPD clients
```

## What works today

**Playback**
- Decoder plugins chosen per file, with fallback: Symphonia (FLAC, MP3, AAC,
  ALAC, Vorbis, WAV…), libopus, and FFmpeg as an isolated process for
  everything else (WavPack, APE, DSD, WMA…).
- Sample-exact **gapless** playback: the next track is decoded ahead and joined
  at the exact sample, with encoder delay and padding trimmed (LAME, iTunSMPB,
  Opus pre-skip). A test plays two halves of one sine wave back to back and
  checks for no discontinuity.
- A lock-free ring buffer between the decode thread and the audio callback.
  The playhead comes from samples actually played, not a timer.
- ReplayGain, with loudness measured on demand for untagged files.
- A restarted engine comes back paused to the same queue, track and position.

**Library**
- Incremental scanning into SQLite with FTS5 search, folder watching, artwork
  extraction and audio hashes, so a track keeps its identity when it moves.
- Sorting, grouping and search run in SQL. The UI fetches only the rows in
  view, so a 50,000-track library never crosses the wire at once.

**Interface** (Svelte 5)
- The music area is arranged by **[layout files](docs/LAYOUT.md)** (KDL):
  track tables, album-grouped lists, a cover gallery, artwork, queue and lyrics,
  placed in resizable splits. Layouts are editable by dragging, and new ones
  install by dropping a file on the window.
- **[Theme files](docs/THEMES.md)**, colour palettes taken from the album art,
  a waveform seek bar, and a spectrum visualiser fed by an FFT in the engine.
- Synced, karaoke-style lyrics from embedded tags, `.lrc` sidecars or LRCLIB.
- A phone layout with a drawer, a full-screen now-playing sheet and touch gestures, each remappable in Settings.

**Remote control and integrations**
- Device pairing by QR code, using ed25519 device keys and scoped, revocable
  tokens (Full, Control playback, View only). Discovery over mDNS.
- An **MPD server**, so existing MPD clients can drive Motif.
- Scrobbling to Last.fm, ListenBrainz and Libre.fm, with an offline queue.
- Discord Rich Presence, plus OS media controls (MPRIS, SMTC, macOS Now
  Playing).

**Android** (in progress): the same engine runs in-process behind the same UI,
with background playback that held up through 30 minutes of forced deep Doze.
The phone's volume keys and Motif's volume are the same control, whichever
device sets it; while the phone controls another engine, the keys set that
engine's volume. It behaves like a good Android audio app: calls and alarms
pause it until they end, another player pauses it for good, unplugging
headphones pauses it, and navigation prompts duck it without touching the
user's volume. System back closes the topmost menu, dialog, screen, sheet or
drawer before it leaves the app.

## Engineering notes

Some of the measured results (details in [docs/PLAN.md](docs/PLAN.md)):

- **Webview performance.** The desktop window is WebKitGTK, which turns off GPU
  rendering on NVIDIA. Profiling at 50k tracks found three causes of dropped
  frames: blurred shadows on scrolling content, a progress clock that forced
  layout on every frame, and composited animations that stalled the GPU
  renderer. Fixing them took the album grid from 12 to 62 fps on CPU rendering
  and cut the cost of playback from 9–43% of a core to about 2%.
- **Engine cost.** 0.1% of a core idle and 0.8% playing resampled FLAC, with no
  underruns while a full library scan reads the same USB drive.
- **Visualiser latency.** Spectrum frames land within about ±20 ms of the
  sound, and the tap costs nothing when no client subscribes.
- **Shared types.** The Rust protocol types generate the TypeScript bindings,
  so the engine and UI can't drift apart.
- **Test-first logic.** The player state machine, parsers and protocol
  mappings are unit-tested through their public interfaces. Audio tests run
  against small generated fixture files.

## Roadmap

Next: Android (finishing now), then peer-to-peer sync between devices over
[iroh](https://iroh.computer) (encrypted, no account), shared jam sessions and
synchronised listen-along, more sources (internet radio, Subsonic/Jellyfin),
and audiophile output (bit-perfect/exclusive, EQ, crossfade). The full plan is
in [docs/PLAN.md](docs/PLAN.md).

## Repository layout

| Path | What |
|---|---|
| `crates/protocol` | Wire types shared by the engine and every client; generates TypeScript |
| `crates/engine` | Playback engine: queue and transport state machine, decoding, audio output |
| `crates/library` | Folder scanning, tags, SQLite, artwork, waveforms |
| `crates/integrations` | OS media controls, scrobbling, Discord, lyrics |
| `crates/server` | WebSocket `/ws` endpoint, pairing, MPD, static web UI hosting |
| `apps/ui` | Svelte 5 UI used by the desktop window, Android and browsers |
| `apps/desktop` | Tauri 2 shell embedding the engine (desktop and Android) |
| `apps/headless` | Engine daemon without a window, for a NAS or Raspberry Pi |

## Building

Prerequisites:
- Rust (stable, 1.90+) via [rustup](https://rustup.rs)
- Node.js 24+
- Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev`
  (see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other platforms)

```bash
npm install
```

Desktop app (starts Vite and the Tauri window):

```bash
npm run tauri dev
```

Web UI against a headless engine, in two terminals:

```bash
cargo run -p headless
```

```bash
npm run dev
```

Then open http://localhost:5173. After `npm run build`, `cargo run -p headless`
serves the built UI itself at http://127.0.0.1:7700.

Checks (run before every commit):

```bash
cargo test --workspace
```

```bash
npm run check
```

```bash
npm test
```

`cargo test -p protocol` also regenerates the TypeScript protocol bindings.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
