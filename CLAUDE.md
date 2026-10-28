# Motif

A music player for collectors: plays anything, desktop (Linux/Windows/macOS) and
Android, remote control, no central server. Architecture and roadmap:
[docs/PLAN.md](docs/PLAN.md). Layout file format: [docs/LAYOUT.md](docs/LAYOUT.md).

Status: phase 1 (desktop player) is done; see the milestones and
measurements in `docs/PLAN.md`. Phase 2 (looks great) is next. Local files play for real (decoder plugins → `pipeline.rs` →
cpal, in `crates/engine/src/audio/`), and the library (`crates/library`) is
scanned from the folders added in Settings. Data lives in the OS data
directory (`MOTIF_DATA_DIR` overrides it, handy for tests).

## Rules

- **Commit whenever a coherent, checked piece of work is done** (all the
  commands below pass), with a clear message. Don't push without being asked.
- **The main branch is `master`**, not `main`.
- **The engine is playback-only.** Never put UI concerns (layouts, themes, view
  state, settings) into `crates/engine`, `crates/protocol` or `crates/server`.
  If something non-playback seems to belong there, explain why and ask first.
- **Layouts only arrange the music area.** The left panel and player bar are
  fixed chrome; appearance lives in client-side settings. Widgets live in
  `apps/ui/src/lib/components/widgets/` and are reused everywhere (layouts and
  the now-playing sheet); layouts only place and configure them.
- Pin new dependencies to exact versions that are at least two weeks old.

## Layout

| Path | What |
|---|---|
| `crates/protocol` | Wire types; `cargo test -p protocol` regenerates `apps/ui/src/lib/protocol/*.ts` (commit them; CI fails if stale) |
| `crates/engine` | Queue and transport state machine (`player.rs` is pure and unit-tested) |
| `crates/server` | WebSocket `/ws` + static UI hosting (loopback only until pairing exists) |
| `apps/desktop` | Tauri 2 shell embedding engine + server (binary `motif`) |
| `apps/headless` | Engine without a window on `127.0.0.1:7700` (binary `motif-headless`) |
| `apps/ui` | Svelte 5 + Vite SPA (no SvelteKit). `lib/layout/` parser, presets and store; `lib/themes.ts`; `lib/settings.svelte.ts` |

## Commands

```bash
npm install
npm run tauri dev          # desktop app with hot reload
cargo run -p headless      # engine for browser testing; then `npm run dev`, open :5173
npm run check              # svelte-check, fails on warnings
npm test                   # UI tests (vitest)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Run all of these before saying work is done; CI runs the same on Linux,
Windows and macOS. If no Rust changed (no `*.rs` or `Cargo.*` files),
skip `cargo clippy` and `cargo test` locally and leave them to CI.

## Testing the UI

- The desktop window is **WebKitGTK**, not Chromium, and they differ. Check
  visual changes in WebKit too: WebKitGTK's MiniBrowser
  (`/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/MiniBrowser <url>`) plus
  `import -window <id>` to capture it, where available.
- Dev builds read URL parameters to set up a state without clicking, e.g.
  `?dev-layout=builtin:details.kdl&dev-theme=paper&dev-play=5&dev-sheet=1`
  (also `dev-art=accent|tint|full`, `dev-drawer`, `dev-settings=interface|library|playback|technical|about`, `dev-player=floating`). See `App.svelte`.
  `?dev-library` shows the generated demo library instead of the real one
  (`dev-play` plays it); `?dev-stress&dev-library=50000` runs the webview
  stress test (results in `docs/PLAN.md`); `dev-stress=scroll` measures plain
  scrolling.
- Performance rules from that test: nothing may run every frame while idle
  (update on a timer only as often as the change is visible, and move things
  with `transform`); no long-running CSS/Web Animations, which stall
  WebKitGTK's GPU renderer for seconds; no
  blurred `box-shadow`/`filter` on things that scroll or sit over scrolling
  content; render only the rows in view. WebKitGTK may draw on the CPU (it
  does on NVIDIA unless the desktop shell's GPU workaround applies), so check
  performance there too.
- Avoid native `<input type="range">` (renders differently in WebKit); use
  `components/Slider.svelte`.
- Vite injects component CSS from JS in dev (`emitCss` only for builds)
  because WebKit's module cache otherwise loses styles across dev-server
  restarts. Don't revert that.
- Phone layout is anything at or below 700px wide: the left panel becomes a
  drawer, row splits stack, the now-playing sheet goes full screen.
