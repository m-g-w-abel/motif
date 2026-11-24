# Decision: the phone's UI stack (#263)

Animations on the phone are sometimes jerky. This doc says what was measured,
what the numbers say, and what the options are. **Needs a decision** (see the
recommendation at the end).

## Part 1: what was measured

`scripts/jank.mjs` drives the phone's real WebView (a Sony XQ-DC54, 60 Hz,
Android's debug build of the app) over DevTools. It loads the dev server's page
(its own origin and storage; never the app's page), taps and swipes with
Android's `input` (real fingers), and for each transition records:

- **frames**: `requestAnimationFrame` intervals while it runs; "late" is a frame
  taking over 1.5 refresh intervals (> 25 ms), "over" is the time beyond one
  interval, summed;
- **a DevTools trace**, summed per cause in the transition's window (ms, over
  all threads, so nested events overlap: read it as where the time went, not as
  a budget): script, style/layout, paint/layers, raster, image decode, GC.

The steps are *open an album*, *swipe back*, *open it again*, *swipe back*,
*sheet expand*, *sheet collapse*, run on a freshly loaded page ("first") and
again straight after ("second"). Three repeats, medians:

| Transition | First: late frames (of ~) / over | Second: late / over | Main causes (first run) |
|---|---|---|---|
| open album | 0 / 43, 0 ms | 0 / 45, 0 ms | script 145, style 24 |
| swipe back | 0 / 54, 0 ms | 0 / 54, 0 ms | script 72, paint 55, raster 48 |
| open it again | 0 / 42, 0 ms | 0 / 43, 0 ms | script 122, style 23 |
| swipe back (2) | 0 / 54, 0 ms | 0 / 54, 0 ms | script 70, paint 48, raster 40 |
| sheet expand | 1 / 41, 34 ms | 1 / 41, 34 ms | script 120, style 31, paint 33 |
| **sheet collapse** | **6 / 51, 167 ms** | **7 / 53, 217 ms** | script 312, raster 177, paint 112, style 59, GC 54 |

### What the numbers say

1. **On a warm WebView the screen changes are smooth** (no late frames), and
   first and second runs are the same. Script compile, image decode and shader
   work show nothing here (the trace has no compile or decode events in any
   window). So the "first time is jerky" feeling is **not reproduced by a fresh
   page load**.
2. **The one transition that drops frames is the sheet collapsing**, in both
   runs: 6-7 late frames, worst 67 ms. A CPU profile of it shows JavaScript is
   small (about 100 ms self time: `getBoundingClientRect`/`getClientRects`
   about 60 ms from measuring the parts, `animate`, `see`); most of the time is
   native (style, paint, raster). The first run's native time was about 220 ms
   higher than the second's (581 vs 357 ms of "program" time in the profile),
   which is the only first-run cost seen.
3. **Cheap fixes tried** (CSS injected into the test page to see what a fix is
   worth before writing it; collapse, median of three, late frames / over):

   | Variant | First | Second |
   |---|---|---|
   | baseline | 6 / 167 ms | 7 / 217 ms |
   | no animated `border-radius` on the sheet's backdrop | 3 / 134 ms | 3 / 117 ms |
   | `will-change: transform, border-radius` on the backdrop | 5 / 167 ms | 6 / 150 ms |
   | hide the tint layer | 4 / 133 ms | 4 / 117 ms |

   Animating `border-radius` on a full-screen element repaints it every frame
   (it is not compositor-driven); removing it halves the late frames. Layer
   hints did nothing; the tint layer is within noise. Nothing here is a clear
   win without a visual change, so no fix was committed. Image pre-decoding and
   `content-visibility` were not tried: image decode is zero in every window,
   and the grids are already windowed (#259).

### What was *not* measured (and why it matters)

- **A cold app start.** The WebView process here was warm. Restarting the app
  (`am force-stop`) would interrupt the user's player, so it was not done. A
  cold renderer (empty JIT, no raster caches, first GPU shader compiles) is the
  likeliest source of the *first-time* jank the user feels. This needs one
  run with the app force-stopped and relaunched, which the user should
  approve (and not while music plays).
- **The user's real library** (thousands of albums, real covers, their engine
  over a network). The test library is 24 albums of generated covers.
- WebKitGTK on the desktop (not the point of this ticket).

## Part 2: options

Sizes: `apps/ui` is about 25,000 lines (13,800 of them in Svelte components,
3,600 of tests); the Rust crates are about 21,800; the Android shell is
about 830 lines of Kotlin plus Tauri's.

| Option | What it is | Cost | Gain | Risk |
|---|---|---|---|---|
| **A. Fix in place** | Stay on Svelte in the system WebView; fix what the traces point at (the radius animation, the measuring in the sheet's collapse), add a cold-start warm-up, keep `scripts/jank.mjs` as a regression check | Days | Removes the measured frame drops; one UI for everything | A WebView has a floor: cold start and first raster are the browser's, not ours |
| **B. Native Android UI (Compose)** over the same `/ws` protocol | Kotlin client, Compose screens, the Rust engine as a library or a local service | 25k lines rewritten for the phone (rough: 15-20k lines of Kotlin for parity), plus Kotlin types generated from the Rust protocol (ts-rs has no Kotlin target: a second generator or hand-kept) | Native scrolling, gestures, transitions; Android conventions (predictive back, shared-element transitions are built in) | Two UIs to keep at parity for every feature (layouts, themes, settings, sync UI, pairing, lyrics...). The phone gets features later than desktop. Remote control and the browser UI stay web |
| **C. Flutter** | One Dart UI for phone (and optionally desktop), own renderer | Larger than B (no shared UI code with Svelte and a new language), Dart bindings for the protocol | Consistent 60/120 fps, shader warm-up tooling, one codebase if desktop moves too | Replaces the whole UI stack, including the web UI the engine serves to browsers (Flutter web is heavy); a third language beside Rust and TS |
| **D. Rust-native UI** (Slint, egui, Dioxus native) | UI in Rust, in the same process as the engine | Largest: immature on Android, accessibility, text input, IME and gestures are weak points | No WebView; the engine and UI share types directly | Least mature option for a phone-grade UI; the desktop and browser UIs would still need the web build |

Shared in every option: `crates/protocol`, the engine, the library and the
server do not change (the protocol is the seam). The desktop app and the
browser UI stay on Svelte under A, B and D; only C asks them to move too.

Maintenance: A keeps one UI. B and D add a second UI that must follow every
feature the first one gets; with a solo developer and a feature list this
long, that is the main cost, more than the rewrite itself. C trades it for a
full migration.

## Recommendation

**Do A, then decide on cold start with data.**

1. The measured problems are specific and small: one transition (the sheet's
   collapse) drops 6-7 frames, mostly from repainting an animated
   `border-radius` and measuring parts during the change. The other
   transitions are smooth on a warm WebView. That is a fix of days, not a
   rewrite.
2. Before choosing a different stack for a first-run problem, run the one
   measurement that is missing: a cold start of the app on the user's
   phone, with their real library. If first-run frames are still bad there
   after the fixes (and a warm-up of the sheet and screens at start), the
   cause is the WebView's cold path and only B or C removes it; then the
   cheaper of those is **B, scoped to the phone's player screens first**
   (now-playing sheet and bar, which are the animated ones), over the same
   protocol, with the Library screens staying web until proven worth moving.
3. D is not recommended for a phone UI today; C only if desktop is also
   going to move.

The user picks: **A now** (recommended), **A and the cold-start measurement**
first, or **B** (say whether the whole phone UI or the player screens first).
