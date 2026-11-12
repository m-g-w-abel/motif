# Motif

A music player for collectors: plays anything, desktop (Linux/Windows/macOS) and
Android, remote control, no central server. Architecture and roadmap:
[docs/PLAN.md](docs/PLAN.md). Layout file format: [docs/LAYOUT.md](docs/LAYOUT.md). Theme file format:
[docs/THEMES.md](docs/THEMES.md). How things move (animations, gestures):
[docs/ANIMATION.md](docs/ANIMATION.md).

Status: phases 1 (desktop player), 2 (looks great), 3 (cheap wins) and 4 (remote control:
pairing, LAN, MPD) are done; see the notes and measurements in `docs/PLAN.md`. Phase 5
(Android) is next. Local files play for real (decoder plugins → `pipeline.rs` →
cpal, in `crates/engine/src/audio/`), and the library (`crates/library`) is
scanned from the folders added in Settings. Data lives in the OS data
directory (`MOTIF_DATA_DIR` overrides it, handy for tests).

## Rules

- **Commit whenever a coherent, checked piece of work is done** (all the
  commands below pass), with a clear message. Don't push without being asked.
- **The main branch is `master`**, not `main`.
- **Keep `README.md` current.** It's a showcase for people we're showing the
  project to (what works, notable engineering, how to build). When a change
  adds or finishes a user-visible feature or a phase, or moves a number it
  quotes, update the README in the same PR. Mention planned streaming sources
  only by their general kind, not by service name.
- **Keep `CHANGELOG.md` current.** A user-visible change adds a line to its
  newest section in the same PR. The app bundles and shows it (Changelog in the
  phone menu and the left panel), so write for users, and name streaming sources
  only by their general kind.
- **The engine is playback-only.** Never put UI concerns (layouts, themes, view
  state, settings) into `crates/engine`, `crates/protocol` or `crates/server`.
  If something non-playback seems to belong there, explain why and ask first.
- **Layouts only arrange the desktop's All Songs view.** The left panel (the
  Library home) and player bar are fixed chrome, other views (`lib/views.svelte.ts`)
  show one widget each, and phones use no layouts; appearance lives in client-side settings. Widgets live in
  `apps/ui/src/lib/components/widgets/` and are reused everywhere (layouts and
  the now-playing sheet); layouts only place and configure them.
- Pin new dependencies to exact versions that are at least two weeks old.

## Writing code and tests

- Shape the data first, and make illegal states unrepresentable (enums and
  types over runtime checks). Commands and handlers should be safe to repeat:
  remote clients retry.
- Logic (state machines, parsers, protocol mappings) is test-first: write the
  failing test, see it fail, then make it pass. Visual changes are checked in
  WebKit instead (see Testing the UI).
- Test behaviour through the public interface. A test that would still pass if
  the function returned nothing or a default value proves nothing: fix it or
  delete it. Refactor only while tests pin the behaviour.
- Don't silence the compiler or linters (`#[allow]`, `@ts-ignore`,
  `svelte-ignore`); fix the cause, or explain in a comment why it can't be.

## Layout

| Path | What |
|---|---|
| `crates/protocol` | Wire types; `cargo test -p protocol` regenerates `apps/ui/src/lib/protocol/*.ts` (commit them; CI fails if stale) |
| `crates/engine` | Queue and transport state machine (`player.rs` is pure and unit-tested) |
| `crates/library` | Library: scan, tags, SQLite (`db.rs` holds sort orders and migrations), artwork, waveforms |
| `crates/integrations` | OS media controls |
| `crates/server` | WebSocket `/ws` + static UI hosting; `pairing.rs` (engine identity, paired devices, scoped tokens; loopback needs none) |
| `apps/desktop` | Tauri 2 shell embedding engine + server (binary `motif`) |
| `apps/headless` | Engine without a window on `127.0.0.1:7700` (binary `motif-headless`) |
| `apps/ui` | Svelte 5 + Vite SPA (no SvelteKit). `lib/layout/` parser, presets and store; `lib/themes.ts` (built-ins), `lib/themeFile.ts` + `lib/userThemes.svelte.ts` (user themes); `lib/sort.svelte.ts`; `lib/settings.svelte.ts` |

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

Android (needs the SDK, NDK and the Rust Android targets; `ANDROID_HOME`,
`NDK_HOME` and `JAVA_HOME` set; libopus's CMake build also needs
`ANDROID_NDK_ROOT`). Debug APK for a 64-bit phone, then install it over USB
(USB debugging on):

```bash
ANDROID_NDK_ROOT=$NDK_HOME npm run tauri android build -- --debug --apk --target aarch64
adb install -r apps/desktop/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

`ANDROID_NDK_ROOT=$NDK_HOME npm run tauri android dev` runs it on the connected phone with hot reload.
On first start, Settings → Library → Allow All files access opens the system
toggle; "Add folder" then uses the system folder picker (phone storage or SD
card). `adb logcat --pid=$(adb shell pidof io.github.mgwabel.motif)` shows the
app's log (the engine's tracing is tagged `RustStdoutStderr`). In a debug build,
`adb forward tcp:9333 localabstract:webview_devtools_remote_$(adb shell pidof io.github.mgwabel.motif)`
opens the WebView to `chrome://inspect` and the DevTools protocol; a locked or
hidden phone pauses it.

`node scripts/landing.mjs [dev-server-url]` checks in headless Chromium that
every shared element of a screen change lands exactly on its real element
(docs/ANIMATION.md, Checking); run it after animation changes, against a muted
test engine with a library (not the user's). With `CDP=http://127.0.0.1:9333`
it runs in the phone's WebView instead (forwarded as below, plus
`adb reverse tcp:5182 tcp:5182` to the dev server).
`node scripts/soak.mjs [mode] [seconds]` soak-tests engine memory (see the
script). Run all of these before saying work is done; GitHub CI is disabled
for now, so these local runs are the only check. If no Rust changed (no
`*.rs` or `Cargo.*` files), `cargo clippy` and `cargo test` can be skipped.
In a new git worktree, run `scripts/seed-build.sh` before the first build: it
copies the main checkout's compiled dependencies (never share one build folder
between checkouts; cargo would mix their workspace crates).
Never use `pkill -f` or `pgrep -f`: the pattern is in the shell's own command
line, so it matches that shell, killing it or never ending a wait loop (which
blocks archiving the session). A hook in `.claude/settings.json` refuses them.
Kill by PID (from `ss -ltnp` or `/proc/<pid>/cmdline`); to wait on a build,
run it in the background and wait for its notification.

## Testing the UI

- The desktop window is **WebKitGTK**, not Chromium, and they differ. Check
  visual changes in WebKit too: WebKitGTK's MiniBrowser
  (`/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/MiniBrowser <url>`) plus
  `import -window <id>` to capture it, where available.
- Dev builds read URL parameters to set up a state without clicking, e.g.
  `?dev-layout=builtin:details.kdl&dev-theme=paper&dev-play=5&dev-sheet=1`
  (`dev-sheet=half|full` opens the phone sheet on that queue step)
  (also `dev-art=accent|tint|full`, `dev-lyrics` (the sheet's lyrics view), `dev-view=albums|songs|playlists|played|added|search|album:<key>|playlist:<id>` (opens a view), `dev-arrange`, `dev-settings=interface|library|playback|keyboard|gestures|integrations|devices|technical|about` (with `dev-pair`, the Pair a device dialog), `dev-player=floating`, `dev-scrub=0.3` (holds the next transition at that progress, to inspect a frozen frame), `dev-engine=host:port&dev-token=…` (adds an engine to switch to)). See `App.svelte`.
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
- Phone layout is anything at or below 700px wide: the left panel goes, the
  Library home and each view fill the music area (one widget each, no
  layouts), the now-playing sheet goes full screen.
