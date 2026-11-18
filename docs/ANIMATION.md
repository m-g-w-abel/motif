# Motion

How Motif animates. The reference for any UI work that moves something: new
pages and features get their animations by declaring them with the tools
below, not by writing animation code.

Status (#180): all of it is built (`apps/ui/src/lib/motion/`:
`transition.ts`, `fit.ts`, `plan.ts`, `change.ts`, `mover.ts`,
`stage.svelte.ts`, `swap.ts`, `surface.ts`); every animation in the app
runs on it.

## Rules

These were decided in `docs/PLAN.md` (UI decisions, Motion) and hold for
everything here:

- Every screen change animates; elements on both screens move to their new
  places.
- Every transition that comes with a motion **follows the finger**, then
  completes or springs back on release. It never waits for the gesture to end
  and then plays.
- Closing mirrors opening: it is the same transition played backwards.
- No harsh changes: nothing jumps, flashes, clips or changes colour or weight
  at once.
- `transform` and `opacity` only (one exception: a clipping box that changes
  size during a change, below), short (260 ms), nothing running while idle,
  no blur on anything that moves or scrolls, no long-running animations
  (they stall WebKitGTK's GPU renderer). Reduced motion respected.

## Why one model

Before this, "move to a new place", "slide in", "fade" and "follow the finger"
were each written several times: Svelte `leave`/`arrive` plus `shared`
flights for screens, a second scrubbed path (`Scrub`, `backScrub`,
`quietly`) for the swipe back only, `follow` for Settings, the now-playing
sheet's own morph (`MORPH`, CSS transitions with computed delays) and step
changes (`ROLES`, `STEPPED`, `FADING`, `stepChange`), `slide`/`swipeRow`
for rows and the bar, and `rise`/`pop`/`dim`/`refresh`/`slideRows`. Each copy
had its own idea of where an element starts, what is drawn while it moves and
when it hands over, so fixing one left the others broken: elements landing in
the wrong place, animations that play after a gesture instead of following
it, and text that clips or changes style abruptly.

The foundation has **one model** (a transition is a function of progress),
**one way to build transitions** (measure, change, measure, diff), **one
mover** for shared elements (screens and sheet alike) and **one driver**
interface for clocks and fingers.

## The model

A **transition** is a set of tracks, each a function of one number, the
progress `p`, from 0 (before) to 1 (after). Nothing else moves anything.

- A **track** is one element, its keyframes (transform and opacity only) and
  the stretch of progress it runs over, its **span** (default `[0, 1]`).
  Tracks are Web Animations, created **paused**; the transition alone sets
  their time.
- Progress has a **clock domain**: time `τ` from 0 to `DURATION`, and
  `p = CURVE(τ)`. Every track lives in time; a span `[a, b]` in progress
  becomes the delay and duration `[CURVE⁻¹(a), CURVE⁻¹(b)] · DURATION`, and
  its easing is that stretch of the curve (`CURVE.segment(a, b)`, itself a
  cubic Bézier), so **every track is linear in progress** within its span.
- **A clock drives it** (a tap, a button, a key): `play(1)` plays the paused
  animations on from where they are. They run on the compositor; no script
  per frame.
- **A finger drives it** (a gesture): `seek(p)` sets every animation's time
  to `CURVE⁻¹(p)`, so what is drawn is exactly `p` of the way: linear with
  the finger. `release(commit)` plays on to 1 or back to 0 from there,
  decelerating along the curve.
- **A clock eases out the way it travels.** Closing mirrors opening in its
  path and its states, not its timing: played backwards, an ease-out curve
  is an ease-in (it starts slowly and arrives fast). So `play(0)` (closing,
  a tap back, a drag let go to spring back) draws the transition with its
  **mirrored tracks** (frames reversed, spans mirrored, which draw the same
  path since each track is linear in progress) and plays them forwards from
  the mirrored progress: it eases out towards 0 as opening eases out towards
  1. A finger still seeks linearly either way.
- **Backwards is free.** Closing is the opening transition built in the same
  direction, seeked to 1 and played to 0. A tap back and a swipe back are the
  same transition, one clocked and one dragged.
- **Interrupting.** Each place things move (a **stage**: the screen area,
  the sheet, a menu) runs one transition at a time. A new gesture on a stage
  with a transition still playing **catches it**: it pauses where it is and
  the finger carries on from that progress (`grab()`; the sheet's
  open/close), or, if the new change is a different one, the running one
  jumps to its end first (the sheet's steps, the screens). Nothing is ignored (no `busy` flags dropping input) and nothing
  restarts from 0.
- **Repeat-safe.** `release` and `play` on a finished or released transition
  do nothing; `done` resolves once. Matches the rule that handlers are safe
  to repeat.
- **Clean-up.** When a transition ends, its animations are cancelled, copies
  removed and inline styles restored, so the layout alone holds the result
  and nothing is left running.
- **Reduced motion:** transitions keep their opacity tracks, drop transforms
  and run in 120 ms; a finger still seeks them.

```ts
// lib/motion/transition.ts
type Span = [number, number];
interface Track { el: HTMLElement; frames: Keyframe[]; span?: Span; easing?: string }

class Transition {
  constructor(tracks: Track[], cleanup?: () => void);
  readonly progress: number;
  seek(p: number): void;                 // finger
  play(to: 0 | 1): Promise<void>;        // clock, from where it is
  release(commit: boolean): Promise<void>; // = play(commit ? 1 : 0), once
  grab(): number | null;                 // a finger catches it playing: pauses where it is drawn
  add(track: Track): void;               // joins where the others are (a card that loads late)
  end(): void;                           // jump to where it was heading, clean up
  readonly done: Promise<void>;          // once, when it has ended and cleaned up
}

/** What a gesture drives. */
interface Drive { seek(p: number): void; release(commit: boolean): void }
```

`Drive` is what `gesture()`, `backSwipe` (Android's predictive back), the
sheet's drags and the step handles all talk to. **Distances are on screen:**
a finger moves in screen pixels (the root's zoom included: the Scale
setting, Android's font size), so a change's `distance` and a swipe's are
screen pixels too (an element's `getBoundingClientRect()`, not its
`offsetWidth`); in CSS pixels, a step dragged at 125% ran ahead of the
finger by the zoom.

**Following the finger: `follow()`.** Every drive goes through
`follow(build, { early, from })` (`transition.ts`), so no gesture can jump.
The transition may still be being built when the finger moves (the screen
beneath laid out, its first data awaited, everything measured), or its build
may have held the main thread while the finger moved on. Meanwhile `early`
draws what it can, exactly with the finger (the screen on top sliding);
the transition then takes over where the drawing was left (else at `from`)
on its first frame and catches up with the finger over `CATCH` (120 ms),
after which it follows it exactly. Letting go plays on from what is drawn,
built or not. Each drive also measures from where the press became a drag,
never from the press, so nothing jumps by the slop. Each turns its distance into
progress (`dx / width`, `dy / distance`) and its commit decision (`Press.swipe`,
`released`, flick velocity) into `release`. Recognising gestures stays in
`gestures.ts`; drawing them lives here. A sideways swipe is a `Swipe`: for
the way the finger first goes that does something, a `Drive` and the
distance it spans (`slide`, `swipeRow`, the swipe back).

## Building transitions: measure, change, measure

Every transition between two states is built the same way, FLIP style:

```ts
const t = change(stage, () => { views.open(album); flushSync(); }, options);
t.play(1);           // a tap
// or: drive = t;    // a gesture seeks it
```

1. **Snapshot** the stage: every element that declares a role (below), with
   its box, its look (for text) and, for those that may leave, a ready copy.
2. **Change** the state and flush, so the DOM is in its final layout.
3. **Snapshot** again.
4. **Plan** (pure, unit-tested): pair elements by name across the two
   snapshots and decide each one's tracks:
   - in both → **move** from the old box to the new (shared element);
     a move always runs the whole transition (it has no span: everything
     that moves starts moving at the start);
   - only after → **enter** as it declares;
   - only before → **leave** as it declares (a copy, since the DOM dropped it);
   - inside something that moves → its own move less its parent's (the
     "lift"), so nested parts don't double up; inside something coming,
     going or flying → nothing of its own, it rides along;
   - the elements between parts and up to the stage (a row holding the
     play button and the skip buttons, the line holding the times) are
     **implicit parts**: kept, they move like any part, so what they hold
     moves with them instead of jumping to its new place; they never come or
     go on their own;
   - a kept part whose move would take it outside a box that clips it (a
     title in its marquee's line, the last row of a list that shrank) travels
     as a **copy** instead, like a shared element, so it is never cut off
     (declared parts only: a whole list moving in its scroller is clipped by
     it, as at rest);
   - a kept box that clips and changes size has its clip animated from its
     old size to its new one, as a `clip-path` polygon in its own coordinates
     (which its move carries), its own clipping set aside until the end: a
     transform moves a clip but can't resize it, so its old clip would be its
     new size in its old place (cutting off what it held, or showing what it
     hid). The one property besides transform and opacity a change animates,
     only there and only for the change's 260 ms. A box scrolled away from
     its start stops being a scroller while its clipping is set aside, so
     what it holds is held at its scroll position by `translate` (apart from
     the transforms a change animates) and the scroll is put back at the end;
     keeping its own clipping instead cut a shrinking one (the full queue
     back to the half) to its new size at once. Copies are scrolled as their
     originals were (`rescroll`);
   - **nothing pops**: an element the change shows that no part declares (a
     part's insides it brings) fades in over the `enter` span, and one it
     takes away while its part stays fades out over the `leave` span, drawn
     from its part's copy made before the change (the snapshot records where
     each drawn element was and where it sits in its part);
   - **every kept element moves**: any drawn element the change keeps moves
     from where it was by its own share (what the nearest kept element it is
     in doesn't already show), so the skip buttons of a row the change made
     narrower slide to their new places; words the change wraps differently
     (a queue row that got wider) crossfade from their old drawing (from
     their part's copy, travelling with them) to their new one. A box's own
     background still takes its new size at once.

The planner works on plain data (names, boxes, nesting, declared enter/leave),
so the rules are tested without a browser. The DOM layer around it only
measures, copies and applies keyframes.

The sheet's open/close is the same with both states always in the DOM: the
"before" snapshot is the player bar (`from`), the "after" is the sheet.
State that changes outside a callback (the queue, from the engine) uses the
two halves: `measure(root)` before, and the function it returns once the
DOM has changed. **One change at a time per stage:** a change drawn on a
stage ends any change still running inside it, starting from where that one
left things drawn (rows still sliding when the next update comes), and a
second `measure` before the first is drawn (two updates in one tick) starts
from the first's snapshot. A part's own transform (a row a reorder has moved
aside) stays under its move.

## Declaring it

Elements opt in with attributes; no per-element code.

| Attribute | Means | Replaces |
|---|---|---|
| `data-move="name"` | A shared element: anything with the same name on the other side is the same thing, and it moves there. Names are unique per side (`cover:album:<key>`, `title:album:<key>`, `title:now`). | `use:shared`, `MORPH` pairs, `ROLES` |
| `data-enter="right\|left\|below\|above\|rise\|fade\|grow"` | How it comes in when it has no counterpart, and (mirrored) how it goes. `rise` is a short lift (40 px) with a fade; `below` is from beyond the stage's bottom edge; `grow` is the dialog's scale-and-fade. | `FADE`, `rising`, `STEPPED` fresh/gone, `LEAVE`, `FADING`, `rise`, `pop` |
| `data-span="name"` | For parts that come or go: the part of the transition they run in, from the stage's named spans (below). Moves have none. | `OUT`, `IN`, `RISE` |
| `data-fit="box\|even\|text\|line"` | How a shared element's old box maps onto its new one (stretch, scale evenly, match the words, stretch across only). Default: `text` for an element with only text, `even` for artwork, else `box`. | `Mode` |

A **stage** is the root a transition is built in, with its named spans:

```ts
const SPANS = { leave: [0, 0.1], enter: [0.6, 1], queue: [0.5, 1], fall: [0, 0.5] };
```

A stage's choreography (that the sheet's controls rise after the movers
arrive) lives in its span table and its markup, not in code. Spans are
only for what comes and goes; everything that moves starts at the start and
runs the whole way (a delayed move sat still, then rushed, and under a
finger it looked like a jump).

Components that only need a small part of this use actions:

- `use:swap={key}`: when `key` changes (a new search, another filter, the
  Songs/Albums switch), what the element holds rises into place. **Only on a
  change of `key`**: not on mount, not when data arrives in pieces. Replaces
  `refresh`.
- `transition:surface={{ enter: 'below' | 'grow' | 'fade' }}`: a Svelte
  `in:`/`out:` adapter for things whose lifetime an `{#if}` owns (menus,
  dialogs, scrims, the selection bar). Its keyframes, duration and curve come
  from the same primitives, so a menu and a screen move alike. Replaces
  `rise`, `pop`, `dim`. These open and close on taps; when one gains a drag
  (a sheet dragged down to close), it moves onto a stage and a `Drive`.

## Primitives

All return tracks; the planner and the actions are built from them.

- **slide(el, from, distance)**: in from or out to a side, by a fraction of
  the stage (screens: 100 % in, 8 % under).
- **fade(el)**, **rise(el, by)**, **grow(el)**: the enters above.
- **dim(el)**: what lies under something arriving darkens (opacity on a
  scrim or the screen beneath, not on the screen on top).
- **move(from, to)**: the shared-element mover (next section).
- **flip(rows)**: rows that stay move from their old place; new rows fade
  in. It is `change()` with each row as `data-move="row:<key>"`, so it is
  not a separate mechanism. Replaces `slideRows`.
- **spans and stagger**: any track takes a span; `stagger(tracks, step)`
  offsets their spans (rows coming in one after another).

## Shared elements: one mover

One implementation for screens, the sheet's morph and the sheet's steps.

**What travels.** The *destination* element, drawn as it will stay. If it
can travel in place (no ancestor between it and the stage clips or scrolls
it, and its stage isn't itself moving), the real element moves by transform.
Otherwise a **copy** travels in the **motion layer** (one fixed layer above
everything, `pointer-events: none`) and the real one is hidden until the copy
lands. Screens always use copies (both screens scroll and clip); the sheet
mostly moves its own elements.

**Where it lands.** The destination box is measured in the final layout
*without* its stage's transform (a screen sliding in is measured where it
will rest), after the new screen has restored its scroll position. A shared
element whose counterpart isn't laid out yet (a grid card that appears once
the grid scrolls to it) joins the transition late with `add`, at the current
progress.

**Text.** Text is the case that kept breaking:

- *One drawing.* The words travel as the new element's drawing, scaled
  evenly and matched by their first letter (not the element's box, so
  padding and alignment don't throw it off). Two drawings of the same words
  never crossfade at different positions (#109).
- *No clipping.* A long title is clipped differently at each end (ellipsis
  in the bar, full width or a marquee in the sheet). The travelling copy sits
  in a **clip box** that moves from the old element's visible box to the new
  one's (its box cut by every box that clips it: a long title's marquee line),
  and the text inside is counter-scaled so it is never squashed. The
  clip box's scale and the text's counter-scale are sampled into keyframes
  (8 steps), since the inverse of a linear scale isn't linear. Transform only.
  The copy is the new element at its own size (its own ellipsis or
  wrapping), so the cut slides open rather than jumping.
- *Colour, weight, cut.* If the two ends differ in colour, weight or font, or
  one is cut off or wrapped and the other isn't, the old drawing travels
  too, locked onto the new words, and the two crossfade across the whole
  move: each end is exactly one drawing and the look changes gradually.
  When both are whole single lines, the two also stretch a little across so
  their words span the same width all the way (bold is wider than regular,
  and would otherwise ghost). Same look: one drawing only.
- *Hand-over.* At the end the clip box is the destination's box and the
  copy is its drawing, so removing the copy and showing the real element in
  the same frame changes nothing.
- *Inline text.* A copy is a block the size of its element. An inline
  element's box is its font's own height, while its line height can be
  taller (the category title's `<h1><span>`: 30 px of Roboto in a 32.76 px
  line), so the copy's line height is set to that box's height; otherwise
  its words sit centred in the taller line, lower, and hop up at the
  hand-over.
- *Zoom.* Copies sit in CSS pixels under the root's `zoom` (the Scale
  setting, and on Android the system font size: `MainActivity.kt` turns the
  WebView's own text zoom off and the page applies the font scale as part of
  its zoom, so what is measured is what is drawn). The mover measures the
  zoom once per value and converts every measured box; the stage does the
  same for a sliding screen's offset.

**Images.** Every cover draws already decoded: `Artwork` remembers the
covers decoded this session (`DecodedArt`), shows one at once on a screen
that mounts, and its `<img>` decodes synchronously (an asynchronous decode
lets the WebView paint a frame without it). The traveller is whichever end's
image is already decoded at the higher resolution (the grid's or the page's, the strip's or the cover's).
Copies of images are made eager and synchronous, and the copy is only shown
once `img.decode()` has resolved (within a frame for a cached image), so a
copy never draws empty; when every image in the copies has loaded already,
the copies show and the originals hide at once, so a real element is never
drawn for a frame at its new place before its copy takes over.

**Stacking.** Each copy travels at its end's place in the stage's stack.
An end that lies behind the stage's content (a negative `z-index` inside
the stage, like the half queue's cover, under the title and its shade)
travels in the stage's **back layer** (`Options.back`: the sheet's
`.behind`, just above its backdrop), so what lies over it at rest lies over
its copy too; every other end travels in the motion layer, above everything.
When the two ends sit at different places (the cover from the carousel to
behind the half queue's title), each end's drawing travels at its own place
and the two crossfade over the move, so neither the start nor the hand-over
changes what is on top. In the motion layer images travel under words, so
a title moving over a cover stays over it.

**Other drawings.** Two ends that aren't the same image and are made
differently (the bar's thin seek strip and the sheet's waveform): the old
drawing travels too, stretched the same way, and the two crossfade over the
whole move, so the waveform isn't there from the start.

**Copies keep their insides' look.** Rules that reach an element's insides
through its ancestors (a phone rule hiding the album's name in the sheet, a
font set on the sheet) don't reach its copy in the motion layer or the
sheet's leaving layer, so a copy (`cloneOf`) carries its insides' display,
visibility, opacity, font, colours, backgrounds and layout (size, margins,
padding, flex and grid) inline, up to 300 elements (a longer list's rows
keep their own rules); a box of `display: contents` stays one. Words on
one line stay on one line (a copy's width is a fraction of a pixel off).
A copy runs no CSS transitions or `@starting-style` fades of its own: it
moves only as its transition draws it. What flies from a part the change
takes away (the half queue's cover) is copied from the snapshot, made while
it was still drawn.

The destination stays hidden until its own image has decoded (at most a
second), then takes over. Artwork never shows a blank or placeholder frame
mid-flight.

## Where it is used

**Screens** (the music area, Settings on phones). The stage renders a short
keyed list of screens, at most the current one and the one under it, instead
of a `{#key}` block with Svelte transitions:

- *Forward* (open an album): the new screen is added on top and slides in
  from the right, opaque; the old one moves 8 % left beneath it (no dimming:
  it would let the player bar under Settings show through). Shared elements
  travel between them.
- *Back* is the exact reverse. A tap back plays it; a swipe back (touch or
  Android's predictive back) drives it. The screen beneath is the real one,
  mounted at the start of the swipe; when the swipe commits it simply
  **stays**, with the same key: no second mount, no `quietly`, no copy of a
  screen swapped for the real one.
- *Across* (the phone's view buttons, same depth): the new screen fades in
  over the old, which stays opaque underneath until it is covered, so what
  lies below the screens (the player bar) never shows through.
- *Settings on phones* covers the bar; its screens are on the same stage
  (above the bar) and move the same way, so swiping between sections
  follows the finger like any other screen.

Each screen restores its own scroll position on mount (albums grid, song
lists), so a swipe back can fly a cover to its row further down. A list's
head (a category's back, icon, title and toolbar; an album's or playlist's
cover and title) scrolls with its list rather than staying fixed above it:
with a fixed head, a card scrolled partly under it took the flying cover
whole and then lost its top under the head when the copy handed over.

**First data.** A screen coming in is laid out, unseen, for a frame, and
then waits (at most 250 ms) while anything in it is `aria-busy`: lists
until their first rows arrive (`SongList`, `AlbumGrid`), an album's or
playlist's page until its songs do. Before, on the phone, the rows arrived
a frame or two after the slide had started and popped in. An album's or
playlist's page always opens at the top (no scroll memory), so what flies
to it lands where it rests; lists (the albums grid, song lists) keep theirs.

**Kept beneath.** After a change forward, the screen it came from stays
mounted, hidden and inert, beneath the new one; back (tap or swipe) to it
starts at once, with nothing to render (rendering it at the swipe's start
cost a 50 ms frame on the phone). A swipe moves the screen on top with the
finger from its first move, even before the transition is built; the
gesture measures from where it became a swipe, not from the press, so
nothing jumps by the slop. Pointer moves only set animation time: shared
elements that haven't come yet are looked for once a frame instead.

**Touches.** Inside views and Settings, every element has `touch-action:
pan-y` (zero specificity, so components keep their own): the browser never
takes a sideways touch for a pan of its own, which sends `pointercancel` and
ends the swipe after its first move. `main`'s `pan-y` alone isn't enough, as
`touch-action` stops at the nearest scroll container. Search's chips scroll
sideways, and desktop's layouts keep wide tables' panning. Once a press is
a sideways swipe, `gesture()` also cancels the browser's own handling of
the touch (`touchmove`), or it ends in a fling and Chromium drops the next
tap while that runs (nothing could be tapped for half a second). The
sheet's drags and its handles' do the same once they move (`ownTouch`), even
on surfaces whose `touch-action` is `none`: Chromium still took them for a
pan and dropped the next tap. The landing check taps a button with a real
finger as each change stops moving to catch this. While two
screens are up, the one that isn't the state's is `inert`; the stage never
sets `pointer-events: none`, which would cancel the finger driving it.

**The now-playing sheet.** Open/close is one transition between the bar and
the sheet snapshots, with the sheet's spans. Dragging the sheet seeks it;
letting go releases it (flick by velocity, otherwise by distance); tapping
the bar plays it. A drag that catches a closing sheet continues from where it
is drawn. The step changes (now playing, half and full queue, lyrics) are
`change()` on the sheet with the step as the state; the handles drive them.
The bar's elements carry the same `data-move` names as the sheet's
(`cover:now`, `title:now`, `artist:now`, `album:now`, `play:now`,
`seek:now`), so the open/close and the steps share the mover; everything
else declares how it comes and goes (`data-enter`), and the choreography
is the sheet's span table (`ExpandedPlayer.svelte`). Parts that a change
takes away leave as copies in the sheet's `.leaving` layer, behind the
rest. The carousel keeps its size while hidden (half queue, lyrics), so the
cover lands on it the moment it shows again; on phones its other images lie
under the cover and go at the very start of a step (`covered`), before the
cover's two drawings crossfade over them. Each step shows one handle
(`handle:now`: under the controls, above the queue, at the top of the full
queue), which moves between steps.

**Menus, dialogs, the selection bar.** `transition:surface`. The scrim dims.

**Rows and the bar's swipes.** A row's swipe is a transition with the row's
slide and its + / × strip; the finger seeks it, release commits or springs
back. "Queued" then plays a second transition (out and back in); "removed"
leaves the row out and lets `flip` close the gap, and its strip closes up
with it (on the same clock, from the bottom), never covering the row moving
into its place. The bar's and artwork's
track change is the same: the old track slides out with the finger and the
next comes in from the other side.

**Lists.** `flip` for anything that reorders or removes rows in place (the
queue). Content that is replaced wholesale (search results, Songs/Albums)
uses `use:swap`. A reorder (`ReorderList`) is a change too: the rows making
way, the dropped row going to its place and a row let go where it started
all glide from where they are drawn, while the dragged row follows the
finger: the change leaves it alone (`still`), and makes no copies, since
nothing leaves a reorder.

## Performance

- Only transform and opacity, on as few layers as possible; copies live only
  for one transition.
- Clocked transitions run on the compositor (Web Animations played, not
  stepped from script). Seeking runs only on pointer moves.
- Nothing per frame while idle; at the end of every transition all
  animations are cancelled.
- No blurred shadows or filters on anything moving; the motion layer has
  none.
- Short Web Animations are fine on WebKitGTK; the stall in PLAN.md came from
  animations that ran for the whole time a track played.
- Check on WebKitGTK and the phone (CLAUDE.md, Testing the UI).

## Migration

Each step leaves the app working, with the checks passing, and deletes what
it replaces.

1. **Core** ✓ (`transition.ts`, `fit.ts`): `Transition` with the clock
   domain, `Drive`, the maps between boxes and the text flight. Tested:
   span to timing, seek and release, interrupting, repeat-safety, the dev
   scrubber, and that every flight lands exactly.
2. **The mover** ✓ (`mover.ts`): copies, the motion layer, the text clip box
   and look crossfade, image decoding and hand-over.
3. **Screens** ✓ (`stage.svelte.ts`): the screen stage and its keyed list;
   `data-move` on the tiles, pages, category headers and song rows; App's
   back and Settings go through it. Deleted `leave`, `arrive`, `follow`,
   `shared`, `flight`, `fly`, `handOver`, `backScrub`, `quietly`, `peek`
   and `backSettling`.
4. **Content swaps** ✓ (`swap.ts`): `use:swap` in Search and the recents
   switch.
5. **Sheet steps** ✓: the pure planner (pairing, enter and leave, nested lift;
   test-first) and `change()`, its first user; attributes on the sheet,
   `change()` per step, handles as drives. Deletes `ROLES`, `STEPPED`, `FADING`, `seen`, `stepChange`,
   `stepDrag`, `busy`, `scrubbing`.
6. **Sheet open/close** ✓: bar and sheet snapshots, the sheet's spans, drags
   as drives. Deletes `MORPH`, `FADE`, `LEAVE`, `pair`, `copy`, `tinted`,
   `measure`, `apply`, `timing` and the CSS-transition path, and
   `PlayerSheet.shown`/`settling` (the sheet's state is `open` and `moving`).
7. **Rows and slides** ✓: `swipeRow` and `slide` as transitions; `flip` for
   the queue (`measure`). Deleted `slideRows`, `Slide`, `Scrub` and
   `refresh` (the bar's song change uses `swap.ts`'s `risen`).
8. **Surfaces** ✓: `transition:surface` for menus, dialogs, scrims and the
   selection bar. Deletes `rise`, `pop`, `dim`; `screens.ts` is gone.

Steps 1–3 come first because they fix the most of the issues below; 5 and 6
fix the sheet's. After step 3 the user tests a debug build on the phone
before the rest (Checking).

## Checking

- **Landing tests, in vitest** (`lib/motion/*.test.ts`): every flight's
  last keyframe puts the copy exactly on the new element's box; text starts
  exactly as the old drawing and is never squashed on the way; two drawings
  of the same words stay over the same words all the way. A move that
  doesn't land, or text that changes style at once, fails here.
- **Landing check, in a browser** (`node scripts/landing.mjs [url]`):
  headless Chromium drives taps and real touch swipes (phone and desktop,
  Search, Settings) and fails if any copy ended off its real element (for
  text, where its words are drawn: a `Range` over them, not the element's
  box) by more than a quarter pixel or with a different colour, weight or size, or if a change left a
  second screen, copies or hidden elements behind. Dev builds record each
  landing in `window.motifLandings` for it. Run it against a muted test
  engine with a library (CLAUDE.md). `STEPS=file.mjs` runs your own steps
  with its helpers, for debugging. The now-playing sheet is in it too: open
  and close by tap and by real drags (and closing from the full queue),
  every step change by its handle's tap and by dragging it; each shared
  part must land on its real element, and closing must put each back on
  the bar's (recorded as `name (back)`, as is a swipe that springs back).
  It needs a track loaded on the engine, paused.
  Each change is also watched frame by frame: anything that moves (a shared
  element or its copy) must move from the start (no span on a move), and
  under a finger the drawn progress must start where the change starts and
  never move more than 0.1 in a frame's time (a dropped frame is slow, not a
  jump). Every step change of the sheet is held at its start (the clock at
  0): whatever is still drawn must be exactly where it was before, which
  catches elements the layout moved that nothing animated. The queue's
  reorder (by the handle, and one let go where it started) and a swipe
  removing a row must glide: no row may move more than 30 px in a frame's
  time. The queue section queues three songs by swiping them on an album
  page and clears the queue at the end.
  **Held frames:** every change of the sheet (each step, by tap and by drag,
  lyrics, open and close, and closing with the title's marquee scrolled) is
  held with its animations' clock stopped and set to its first, a middle and
  its last frame. The first must look like the screen before it and the last
  like the screen after it, pixel for pixel within a CSS pixel (nothing pops
  in, vanishes or swaps places in the stack as it starts or hands over);
  what is drawn both before and after must be drawn in the middle (a cover
  whose copy drew nothing); and nothing may change in the half second after
  it settles. Marquees, which scroll on their own clock, are waited out
  first. `DUMP=dir` saves a failing case's frames. On the phone the frames
  are the display itself (`adb screencap`, without its system bars, which
  change on their own): the WebView's DevTools capture draws animated layers
  wrongly while the animations' clock is stopped (a frozen first frame that
  the display shows exactly as before, 0 pixels off, it captured 32,000
  device pixels off). Differences are counted in CSS pixels. The display
  shows what the OS draws on top too: a notification arriving during a case
  fails it (seen once each on two runs); run it again.
  **A finger followed one to one:** at every zoom, a change's distance and a
  swipe's must be the screen distance of what moves (a test stage in the page).
  **Undeclared elements:** a test stage built in the page with `change()`
  itself fails if an element no part declares vanishes or pops when a change
  takes it away or brings it.
- **At another zoom and font:** the phone scenarios run again at Scale
  125% (`?dev-scale=125`), which caught boxes measured on screen but placed
  in CSS pixels, and in DejaVu Sans (`?dev-font=`), whose glyphs, like
  Roboto's on phones, sit in their line differently from the default font's
  (headless Chromium's default hid the inline text bug).
- **The same check on the phone** (`CDP=http://127.0.0.1:9333 node
  scripts/landing.mjs http://localhost:5182`): drives the app's own WebView
  (a debug build, its DevTools forwarded, `adb reverse` to a dev server on a
  muted test engine) at the phone's real size, pixel ratio and font scale,
  swiping through Android's input. It caught what desktop didn't: titles
  landing off under the font scale, and swipes the WebView cancelled. The
  WebView goes back to the app's own page at the end; the test page's
  setup never touches the app's storage. Scripts over the phone's DevTools
  act only on the separate test page, never on the app's own page.
- **Frame rates** (2026-11-13, the phone, 150 albums, rAF intervals over
  four rounds): tap open and swipe back run at 60 fps with at most one
  dropped frame; the swipe back's 50 ms first frame (rendering the screen
  beneath) is gone.
  The now-playing sheet (2026-11-16, four rounds each, 700 ms from the
  start): opening by tap averages 16.7–17.2 ms per frame (at most one frame
  over 20 ms, once 38 ms); every step change (half, full, back, lyrics, by
  tap or drag), opening by drag and closing by tap run at 16.7 ms with none
  over 20 ms. Closing by dragging now playing down drops 2–4 frames
  (33 ms, once 67 ms): one long frame with no script and little layout, as
  the transition first takes hold under the finger; to look into.
  After the motion fixes (2026-11-16, four rounds, mean/max ms, frames over
  20 ms): opening by tap 16.8–16.9/22–23/1; the step changes by tap
  16.7–17.1/17–34/0–1; dragging to the half queue and back 16.8/19–21/0–1;
  opening by drag and closing by Escape 16.7/17–18/0. Closing by drag still
  drops frames in three rounds of four (18.5–19.0/34–50/2–5). Two causes,
  seen with long-animation-frame entries: the first move builds the
  transition (about 20 ms of script: the two snapshots about 8, the flights
  about 12, then the sheet's re-render), and sometimes a 50–67 ms frame
  with no script, painting the copies for the first time. Nothing jumps
  there any more (`follow` starts from what is drawn), but the frames drop.
  After the stacking, easing and no-pop round (2026-11-17, same method):
  opening by tap 16.9–17.3/23–40/1; the step changes by tap 16.7–17.2/17–34
  with at most one frame over 20 ms; the step drags 17.1–17.5/33–34/1–2;
  lyrics 16.8–17.2/20–37/1, back 16.7/17/0; opening by drag 16.7–17.1/17–34/
  0–1; closing by Escape 16.7/17/0; closing by drag 18.0–18.5/33–50/2–4.
  The step changes now drop a frame where they dropped none: the snapshot
  does more (every drawn element listed for the default fade, and copies
  carry about 40 properties of their insides), about a frame's work at the
  start; to look into (narrow the drawn list to the parts' children).
  Narrowed (2026-11-17): containers between parts are only measured, a part
  inside a copied part is taken from that copy, and copies read box, flex
  and item properties only where they apply. Building a step change at 6x
  CPU throttle (headless): 72–117 ms before, 59–94 ms after. On the phone the
  build now takes 27–40 ms (copies of the parts 5–14, Svelte applying the
  step 3–10, the flights about 5); frames: step taps 16.7–17.1/17–34/0–1,
  step drags 17.1–17.2/33–34/1, lyrics 16.8/19–21/0–1, lyrics back
  16.7–17.1/17–33/0–1, opening by tap 16.8–17.3/22–39/1, by drag
  17.1–17.6/33–34/1–2, closing by Escape 16.7/17/0, by drag 18.5–19.5/34–67/3–5.
  A tap still drops a frame about half the time: the "before" snapshot
  (mostly its copies, which are needed only for parts the change takes away,
  unknown until it happens) runs inside the tap. Taking it at pointer-down,
  before the click arrives, would move it out of the frame; to do.
  With every kept element moving and rewrapped words crossfading (same day):
  step taps 16.7–17.1/17–34/0–1, step drags 16.7–17.2/17–34/0–2, lyrics
  16.7–17.2/17–36/0–1, back 16.7/17/0, opening by tap 16.8–17.2/21–38/1–2,
  by drag 16.7–17.1/17–34/0–1, closing by Escape 16.7/17/0, by drag
  18.5–19.0/33–50/3–4. The build in the tap: 27–39 ms (the narrower
  snapshot's saving spent on the new moves).
- **A frozen frame** (`?dev-scrub=0.3`, dev builds): the next transition
  stops at that progress instead of playing, to inspect on the phone or in
  WebKitGTK; the next change ends it.
- **The phone**: after steps 3, 6 and 8, a debug build on the phone before
  going on.

## Open issues (fab0560) and what fixes them

| # | Issue | Fixed by |
|---|---|---|
| 1 | Back from an album to Search makes Search jitter | ✓ **Foundation** (the landing check caught it: covers landed 12 px off): the screen beneath stays mounted (no second mount after the swipe) and `use:swap` doesn't fire when results arrive. Search's results run an effect on every piece of data that arrives (`refresh` in `SearchView.svelte`), which a remount sets off. |
| 2 | Opening an album from Search flashes its cover | ✓ in Chromium (no frame without a drawn cover); the phone to confirm. **Foundation**: the mover waits for the copy's image to decode and keeps the page's hidden until its own has. To confirm once moved over; if Search's cover uses a different image size than the page, it may need the traveller rule tuned. |
| 3 | Search's animations restart | ✓ **Foundation**: same cause as 1; `use:swap` animates only on a new search or filter. |
| 4 | Recently Added forgets it was showing Albums | ✓ **Own fix**: `show` is local state in `ViewScreen.svelte`, lost when the screen unmounts; it belongs in the view's saved state. |
| 5 | An old bar at the top of the phone screen | ✓ **Own fix**: the view's `header.toolbar`, gone on phones; the back button sits above a category's title, the home has a title with its ⋮, Playlists' + sits by its title (a playlist being edited keeps the bar for Done). |
| 6 | Settings sections flash the player bar; the move plays only after the gesture | ✓ **Foundation**: Settings screens move on the screen stage, driven by the finger like any back; an across change keeps the old screen opaque beneath, so the bar can't show through (both screens currently fade at once over it). |
| 7 | Long titles clip on the now-playing screen; colour and weight change abruptly | ✓ **Foundation**: the mover's clip box and look crossfade. A title wrapped over two lines (desktop) is an inline-block, so its copy wraps the same. |
| 8 | The seek bar doesn't animate to the right place | ✓ **Foundation**: the bar's strip and the sheet's seek bar are one `data-move` pair (`seek:now`), travelling as a box (with `line` fit, the sheet's taller waveform jumped to its full height at the start). The sheet's slider was not stale on opening: both get the same position and the same timer. |
| — | Song lists don't remember their scroll | ✓ **Own fix** (needed by the foundation's back to a row's cover): `SongList` gets `scrollMemory` like the albums grid and `TrackList`. |
