<script lang="ts">
  import { getApp, setViewState } from '../../context';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ArtworkWidget from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Lyrics from '../widgets/Lyrics.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';

  const { layout, sheet } = getApp();
  let body = $state<HTMLElement>();
  let section = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  let tint = $state<HTMLElement>();
  setViewState({ source: 'library', layoutId: 'now-playing', browse: layout.browseState('now-playing') });

  /**
   * The morph runs in stages so no two moving things cross: the bar's own
   * controls fade out where they are (OUT), the artwork, title and artist
   * travel together (MOVE), then the sheet's controls fade in where they
   * belong (IN). Bar element → its counterpart here, the box to match (`box`
   * stretches, `even` scales evenly, `text` matches the first letter), and
   * what it does:
   * - `move`: the sheet's own element travels; nothing to hand over.
   * - `cross`: a copy of the bar's (a ghost) travels with the sheet's and they
   *   crossfade once both have arrived, drawn at the same place and size.
   * - `handover`: like `cross`, but the contents differ (the bar's line has
   *   the album too), so the ghost is gone before the sheet's appears.
   * - `stage`: the bar's fades out in OUT, the sheet's fades in in IN.
   * - `edge`: the bar's strip rides the top edge of the growing background
   *   and fades; the sheet's seek bar fades in in IN.
   */
  const MORPH: [string, string, Mode, Kind][] = [
    ['.art', '.frame', 'box', 'move'],
    ['.label .title', '.info h1', 'text', 'cross'],
    ['.label .sub', '.info .hero-artist', 'text', 'handover'],
    ['.seek .slider', '.controls .seek .slider', 'box', 'edge'],
    ['.transport', '.controls .transport', 'box', 'stage'],
    ['.time', '.controls .seek .time:first-child', 'box', 'stage'],
    ['.total', '.controls .seek .time:last-child', 'box', 'stage'],
    ['[aria-label^="Repeat"]', '.controls [aria-label^="Repeat"]', 'box', 'stage'],
    ['.volume', '.controls .volume', 'box', 'stage'],
    ['.end > [aria-label="Expand player"]', 'header [aria-label="Collapse"]', 'box', 'stage'],
  ];
  /** No counterpart in the bar: fade in in IN (the carousel's side cards and controls after its cover). */
  const FADE = 'header .handle, header .lyrics-toggle, .art.lyrics, .hero-album, .coverflow .card:not(.frame), .coverflow .turns, .coverflow .below';

  type Mode = 'box' | 'even' | 'text';
  type Kind = 'move' | 'cross' | 'handover' | 'stage' | 'edge';
  /** A stretch of progress. */
  type Span = [number, number];
  /** The background grows until the movers arrive, so they never poke out of it. */
  const GROW: Span = [0, 0.85];
  const OUT: Span = [0, 0.1];
  const MOVE: Span = [0.1, 0.85];
  const IN: Span = [0.85, 1];
  /** Text under the artwork (phones) waits until the growing artwork has risen past it. */
  const MOVE_UNDER: Span = [0.4, 0.85];
  const DURATION = 260;
  const CURVE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';

  /**
   * `el` is moved over `move` by (dx, dy, sx, sy): the sheet's own parts start
   * at the bar and arrive at rest, ghosts start at rest (over the bar) and
   * arrive at the sheet. It fades in over `show` or out over `hide`.
   */
  type Part = { el: HTMLElement; ghost: boolean; dx: number; dy: number; sx: number; sy: number; move?: Span; show?: Span; hide?: Span };
  const STILL = { dx: 0, dy: 0, sx: 1, sy: 1 };
  // Plain, not state: measured once per gesture, read by the effect below.
  let parts: Part[] = [];
  /** The progress last drawn, where a settle's transitions start from. */
  let drawn = 0;
  /** MOVE, or MOVE_UNDER for text, set per measure. */
  let textMove = MOVE;
  let ghosts = $state<HTMLElement>();

  function shown(root: Element, selector: string) {
    return [...root.querySelectorAll<HTMLElement>(selector)].find((e) => e.offsetWidth > 0);
  }

  /** The box of the first letter or digit, or of the element without one. */
  function glyph(el: HTMLElement): DOMRect {
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const i = n.textContent?.search(/[\p{L}\p{N}]/u) ?? -1;
      if (i < 0) continue;
      const range = document.createRange();
      range.setStart(n, i);
      range.setEnd(n, i + 1);
      return range.getBoundingClientRect();
    }
    return el.getBoundingClientRect();
  }

  /** FLIP: the transform (origin top left) that puts box `x`, inside `e`, over box `y`. */
  function fit(e: DOMRect, x: DOMRect, y: DOMRect, mode: Mode) {
    let sx = y.width / x.width;
    let sy = y.height / x.height;
    if (mode === 'text') sx = sy;
    else if (mode === 'even') sx = sy = Math.min(sx, sy);
    const dx = y.left + y.width / 2 - e.left - (x.left + x.width / 2 - e.left) * sx;
    const dy = y.top + y.height / 2 - e.top - (x.top + x.height / 2 - e.top) * sy;
    return { dx, dy, sx, sy };
  }

  /** A copy of the bar's `a`, laid over it here. */
  function copy(a: HTMLElement, selector: string, at: DOMRect) {
    const r = a.getBoundingClientRect();
    const el = a.cloneNode(true) as HTMLElement;
    // Nested parts (the total inside the bar's time) are copied on their own.
    for (const [from] of MORPH) if (from !== selector) for (const n of el.querySelectorAll(from)) n.remove();
    Object.assign(el.style, {
      position: 'absolute',
      left: `${r.left - at.left}px`,
      top: `${r.top - at.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
      margin: '0',
      transformOrigin: '0 0',
    });
    ghosts?.append(el);
    return el;
  }

  /** The parts that carry bar element `a` to sheet element `b`. */
  function pair(a: HTMLElement, from: string, b: HTMLElement, mode: Mode, kind: Kind, at: DOMRect, o: DOMRect): Part[] {
    const ra = a.getBoundingClientRect();
    const rb = b.getBoundingClientRect();
    const [ga, gb] = mode === 'text' ? [glyph(a), glyph(b)] : [ra, rb];
    b.style.transformOrigin = '0 0';
    const move = mode === 'text' ? textMove : MOVE;
    const there = { el: b, ghost: false, ...fit(rb, gb, ga, mode), move };
    if (kind === 'move') return [there];
    if (kind === 'stage') return [{ el: b, ghost: false, ...STILL, show: IN }, { el: copy(a, from, at), ghost: true, ...STILL, hide: OUT }];
    if (kind === 'edge') {
      // Keeps its place relative to the background (stretching only across).
      const sx = at.width / o.width;
      const dx = at.left + (ra.left - o.left) * sx - ra.left;
      const dy = at.top + (ra.top - o.top) * (at.height / o.height) - ra.top;
      return [
        { el: b, ghost: false, ...STILL, show: IN },
        { el: copy(a, from, at), ghost: true, dx, dy, sx, sy: 1, move: GROW, hide: [0.3, 0.5] },
      ];
    }
    const ghost = { el: copy(a, from, at), ghost: true, ...fit(ra, ga, gb, mode), move };
    return [
      { ...there, show: IN },
      { ...ghost, hide: kind === 'cross' ? IN : [0.75, 0.85] },
    ];
  }

  /**
   * Measures where each part starts (in the bar or floating player) and ends
   * (here, untransformed), then pins them at the current progress without
   * easing, so the next change animates from there. Without a player to grow
   * out of, or with reduced motion, the whole sheet fades instead.
   */
  function measure() {
    if (!section || !backdrop || !ghosts) return;
    for (const { el } of parts) {
      el.style.transition = 'none';
      el.style.transform = '';
      el.style.opacity = '';
    }
    ghosts.replaceChildren();
    parts = [];
    const origin = sheet.origin;
    if (origin && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const at = section.getBoundingClientRect();
      const o = origin.getBoundingClientRect();
      const bd = backdrop.getBoundingClientRect();
      const art = section.querySelector('.frame')?.getBoundingClientRect();
      const info = section.querySelector('.info')?.getBoundingClientRect();
      textMove = art && info && info.top >= art.bottom ? MOVE_UNDER : MOVE;
      const found: Part[] = [{ el: backdrop, ghost: false, ...fit(bd, bd, o, 'box'), move: GROW }];
      for (const [from, to, mode, kind] of MORPH) {
        const b = shown(section, to);
        if (!b) continue;
        const a = shown(origin, from);
        found.push(...(a ? pair(a, from, b, mode, kind, at, o) : [{ el: b, ghost: false, ...STILL, show: IN }]));
      }
      for (const el of section.querySelectorAll<HTMLElement>(FADE)) found.push({ el, ghost: false, ...STILL, show: IN });
      // The queue rises from below the page once the artwork and seek strip are clear of it.
      const queue = section.querySelector<HTMLElement>('.queue-slot');
      if (queue) found.push({ el: queue, ghost: false, ...STILL, dy: at.bottom - queue.getBoundingClientRect().top, move: [0.5, 1] });
      parts = found;
    }
    apply(sheet.progress, true);
    // Commit the start styles before the transition to the new progress.
    void getComputedStyle(backdrop).transform;
  }

  function ramp(p: number, [a, b]: Span) {
    return Math.min(1, Math.max(0, (p - a) / (b - a)));
  }

  /**
   * Duration and delay that put span `s` where it falls in a settle from
   * `from` to `to`, as if progress moved evenly over the settle.
   */
  function timing([a, b]: Span, from: number, to: number) {
    const lo = Math.max(a, Math.min(from, to));
    const hi = Math.min(b, Math.max(from, to));
    const span = Math.abs(to - from);
    if (hi <= lo || !span) return '0s';
    const ms = (v: number) => `${Math.round((v / span) * DURATION)}ms`;
    return `${ms(hi - lo)} ${to > from ? ms(lo - from) : ms(from - hi)}`;
  }

  function apply(p: number, instant: boolean) {
    if (!section || !tint) return;
    const from = drawn;
    drawn = p;
    for (const { el, ghost, dx, dy, sx, sy, move, show, hide } of parts) {
      const fade = show ?? hide;
      if (instant) el.style.transition = 'none';
      else {
        // Duration first, then the curve, then the delay.
        const time = (s: Span, curve: string) => timing(s, from, p).replace(' ', ` ${curve} `);
        el.style.transition = [move && `transform ${time(move, CURVE)}`, fade && `opacity ${time(fade, 'linear')}`]
          .filter(Boolean)
          .join(', ');
      }
      if (move) {
        const t = ghost ? ramp(p, move) : 1 - ramp(p, move);
        el.style.transform = t ? `translate(${dx * t}px, ${dy * t}px) scale(${1 + (sx - 1) * t}, ${1 + (sy - 1) * t})` : '';
      }
      if (show) el.style.opacity = String(ramp(p, show));
      else if (hide) el.style.opacity = String(1 - ramp(p, hide));
    }
    section.style.opacity = parts.length ? '' : String(p);
    tint.style.transition = instant ? 'none' : `opacity ${timing(GROW, from, p)}`;
    tint.style.opacity = parts.length ? String(1 - ramp(p, GROW)) : '0';
  }

  sheet.measure = measure;
  // Writes styles only; reads no state it writes.
  $effect(() => apply(sheet.progress, sheet.dragging));

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && sheet.progress === 1) sheet.collapse();
  }
</script>

<svelte:window onkeydown={onKey} />

<!--
  Morphs out of the player bar (or floating player): the background grows from
  its box and the artwork, title, transport and seek bar move from their places
  in it, all by `progress`, so dragging scrubs the morph. Built from the same
  widgets layouts use.
-->
<section
  bind:this={section}
  class="sheet"
  class:open={sheet.open}
  class:dragging={sheet.dragging}
  aria-label="Now playing"
  aria-hidden={!sheet.open}
  inert={!sheet.open}
>
  <div class="backdrop" bind:this={backdrop}><div class="tint" bind:this={tint}></div></div>
  <div class="ghosts" bind:this={ghosts} inert aria-hidden="true"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header onpointerdown={(e) => sheet.drag(e, 'sheet')}>
    <span class="handle"></span>
    <button
      class="icon-button lyrics-toggle"
      class:active={sheet.lyrics}
      aria-label="Lyrics"
      aria-pressed={sheet.lyrics}
      title={sheet.lyrics ? 'Show artwork' : 'Show lyrics'}
      onclick={() => (sheet.lyrics = !sheet.lyrics)}
    >
      <Icon name="lyrics" size={18} />
    </button>
    <button class="icon-button" aria-label="Collapse" title="Collapse (Esc)" onclick={() => sheet.collapse()}>
      <Icon name="chevron-down" size={20} />
    </button>
  </header>

  <div class="body" bind:this={body}>
    <div class="now">
      <div class="art" class:lyrics={sheet.lyrics}>
        {#if sheet.lyrics}<Lyrics />{:else}<ArtworkWidget expands={false} playing />{/if}
      </div>
      <div class="details">
        <TrackInfo style="hero" playing />
        <Controls />
      </div>
    </div>
    <div class="queue-slot">
      <Queue collapsible />
    </div>
  </div>
  <Scrollbar target={body} />
</section>

<style>
  .sheet {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    /* Hidden once the collapse has finished; the fade is the reduced-motion one. */
    visibility: hidden;
    transition:
      opacity 150ms linear,
      visibility 0s 260ms;
  }

  .sheet.open {
    visibility: visible;
    transition: opacity 150ms linear;
  }

  .sheet.dragging {
    transition: none;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    background: var(--bg);
    transform-origin: 0 0;
    will-change: transform;
  }

  /* The bar's colour, fading into the sheet's as it grows. */
  .tint {
    position: absolute;
    inset: 0;
    background: var(--surface);
    opacity: 0;
  }

  /* Copies of the bar's parts, on top while they crossfade. */
  .ghosts {
    position: absolute;
    inset: 0;
    z-index: 1;
    pointer-events: none;
  }

  /* Let the artwork travel to the bar instead of being clipped by its box. */
  .art :global(.artwork) {
    overflow: visible;
    background: none;
  }

  header {
    position: relative;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    height: 40px;
    padding: 0 var(--space-2);
    flex: none;
    touch-action: none;
    cursor: grab;
  }

  .handle {
    position: absolute;
    left: 50%;
    top: 8px;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--border);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* Artwork beside the details; together they take whatever the queue leaves. */
  .now {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(300px, 460px) minmax(0, 0.4fr);
    align-items: center;
    gap: var(--space-6);
    padding: 0 var(--space-6) var(--space-6);
  }

  .art {
    height: 100%;
    min-height: 0;
  }

  /* Lyrics are as large as the space allows, not square like the artwork. */
  @media (min-width: 701px) {
    .now:has(.art.lyrics) {
      grid-template-columns: minmax(0, 1.5fr) minmax(300px, 460px) minmax(0, 0.2fr);
    }
  }

  .art.lyrics {
    align-self: stretch;
  }

  .art.lyrics :global(.lyrics) {
    background: none;
  }

  .lyrics-toggle.active {
    color: var(--accent);
  }

  .details {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  /* The queue sits underneath: a header when folded, ~40% of the height when open. */
  .queue-slot {
    flex: none;
    border-top: 1px solid var(--divider);
  }

  .queue-slot:has(:global(.queue.open)) {
    flex: 0 1 40%;
    min-height: 160px;
  }

  /* Phones: cover the whole screen; artwork, details and queue stack and scroll. */
  @media (max-width: 700px) {
    .sheet {
      position: fixed;
      z-index: 45;
      padding-top: env(safe-area-inset-top);
      padding-bottom: env(safe-area-inset-bottom);
    }

    .body {
      /* Holds the bars of the scroll areas inside it, so they scroll with it. */
      position: relative;
      overflow-y: auto;
    }

    .now {
      flex: none;
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
      padding: 0 var(--space-4) var(--space-4);
    }

    .art {
      height: min(55vh, calc(100vw - 2 * var(--space-4)));
    }

    /* Lyrics are the main view: most of the screen, the controls below. */
    .art.lyrics {
      height: calc(100dvh - 320px);
      min-height: 240px;
    }

    .queue-slot:has(:global(.queue.open)) {
      flex: none;
      height: 60vh;
    }
  }
</style>
