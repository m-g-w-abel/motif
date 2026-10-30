# Motif

A music player for collectors and audio enthusiasts: plays anything, looks
great, runs on desktop and Android, can be controlled remotely, and needs no
central server. See [docs/PLAN.md](docs/PLAN.md) for the architecture and roadmap.

> Phases 1 (desktop player) and 2 (looks great) are done; Android, remote
> control and sync come later. See the roadmap in [docs/PLAN.md](docs/PLAN.md).

## Layout

| Path | What |
|---|---|
| `crates/protocol` | Wire types shared by the engine and every client; generates TypeScript |
| `crates/engine` | Playback engine (queue + transport state machine, decoding, audio output) |
| `crates/library` | Music library: folder scanning, tags, SQLite, artwork, waveforms |
| `crates/integrations` | OS media controls |
| `crates/server` | WebSocket `/ws` endpoint and static web UI hosting |
| `apps/ui` | Svelte 5 UI used by the desktop window, Android and browsers, built from a [layout file](docs/LAYOUT.md), themed by [theme files](docs/THEMES.md) |
| `apps/desktop` | Tauri shell embedding the engine |
| `apps/headless` | Engine daemon without a window |

## Prerequisites

- Rust (stable, 1.90+) via [rustup](https://rustup.rs)
- Node.js 24+
- Linux: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev`
  (see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other platforms)

## Development

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

Then open http://localhost:5173. `cargo run -p headless` after `npm run build`
serves the built UI itself at http://127.0.0.1:7700.

Tests, and regenerating the TypeScript protocol bindings after changing
`crates/protocol`:

```bash
cargo test --workspace
```

UI checks and tests:

```bash
npm run check
```

```bash
npm test
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
