<script lang="ts">
  import { getApp, setViewState } from '../../context';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ArtworkWidget from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';

  const { layout, sheet } = getApp();
  let body = $state<HTMLElement>();
  let section = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  let tint = $state<HTMLElement>();
  setViewState({ source: 'library', layoutId: 'now-playing', browse: layout.browseState('now-playing') });

  /**
   * Bar/floating element → its counterpart here. `box` stretches to fit,
   * `even` scales evenly (buttons), `text` matches the first letter's size and
   * place. Unless the `fade` is `none`, a copy of the bar's element (a ghost)
   * travels along and hands over to the sheet's, so differing fonts, sizes and
   * ellipses don't jump: `cross` crossfades, `swap` switches halfway (the two
   * are drawn in the same box, so only one is ever visible), `late` fades the
   * ghost out early and the sheet's in late, so there are never two at once.
   */
  const MORPH: [string, string, Mode, Fade][] = [
    ['.art', '.frame', 'box', 'none'],
    ['.label .title', '.info h1', 'text', 'cross'],
    ['.label .sub', '.info .hero-artist', 'text', 'cross'],
    ['.transport', '.controls .transport', 'even', 'cross'],
    ['.seek .slider', '.controls .seek .slider', 'box', 'swap'],
    ['.time', '.controls .seek .time:first-child', 'text', 'late'],
    ['.total', '.controls .seek .time:last-child', 'text', 'late'],
    ['[aria-label^="Repeat"]', '.controls [aria-label^="Repeat"]', 'even', 'cross'],
    ['.volume', '.controls .volume', 'even', 'cross'],
    ['.end > [aria-label="Expand player"]', 'header .icon-button', 'even', 'cross'],
  ];
  /** The handle (and anything the bar doesn't show) fades in once the background has mostly grown. */
  const FADE = 'header .handle';
  const EASE = '260ms cubic-bezier(0.2, 0.8, 0.2, 1)';
  /** Opacity transitions when settling, given where the element ends (1 shown, 0 not). */
  const OPACITY: Record<Fade, (r: number) => string> = {
    none: () => 'opacity 0s',
    cross: () => `opacity ${EASE}`,
    swap: () => 'opacity 0s linear 130ms',
    // Leaving goes first; arriving waits until the other has gone.
    late: (r) => (r ? 'opacity 90ms linear 170ms' : 'opacity 90ms linear'),
  };

  type Mode = 'box' | 'even' | 'text';
  type Fade = 'none' | 'cross' | 'swap' | 'late';
  /**
   * `el` is moved by `t` of (dx, dy, sx, sy): the sheet's own parts are at the
   * bar when `t` = 1 - progress, ghosts reach the sheet when `t` = progress.
   */
  type Part = { el: HTMLElement; dx: number; dy: number; sx: number; sy: number; ghost: boolean; fade: Fade };
  // Plain, not state: measured once per gesture, read by the effect below.
  let parts: Part[] = [];
  let fades: HTMLElement[] = [];
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

  /** A copy of the bar's `a`, laid over it here so it can travel to `b`. */
  function ghost(a: HTMLElement, b: HTMLElement, mode: Mode, fade: Fade, at: DOMRect): Part {
    const r = a.getBoundingClientRect();
    const el = a.cloneNode(true) as HTMLElement;
    // Nested parts (the total inside the bar's time) travel as their own ghosts.
    for (const [from] of MORPH) for (const n of el.querySelectorAll(from)) n.remove();
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
    const inner = mode === 'text' ? [glyph(a), glyph(b)] : [r, b.getBoundingClientRect()];
    return { el, ...fit(r, inner[0], inner[1], mode), ghost: true, fade };
  }

  /**
   * Measures where each part starts (in the bar or floating player) and ends
   * (here, untransformed), then pins them at the current progress without
   * easing, so the next change animates from there. Without a player to grow
   * out of, or with reduced motion, the whole sheet fades instead.
   */
  function measure() {
    if (!section || !backdrop || !ghosts) return;
    for (const el of [...parts.map((p) => p.el), ...fades]) {
      el.style.transition = 'none';
      el.style.transform = '';
      el.style.opacity = '';
    }
    ghosts.replaceChildren();
    parts = [];
    fades = [...section.querySelectorAll<HTMLElement>(FADE)];
    const origin = sheet.origin;
    if (origin && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const at = section.getBoundingClientRect();
      const o = origin.getBoundingClientRect();
      const bd = backdrop.getBoundingClientRect();
      const found: Part[] = [{ el: backdrop, ...fit(bd, bd, o, 'box'), ghost: false, fade: 'none' }];
      for (const [from, to, mode, fade] of MORPH) {
        const b = shown(section, to);
        if (!b) continue;
        const a = shown(origin, from);
        if (!a) {
          fades.push(b);
          continue;
        }
        const e = b.getBoundingClientRect();
        const inner = mode === 'text' ? [glyph(b), glyph(a)] : [e, a.getBoundingClientRect()];
        b.style.transformOrigin = '0 0';
        found.push({ el: b, ...fit(e, inner[0], inner[1], mode), ghost: false, fade });
        if (fade !== 'none') found.push(ghost(a, b, mode, fade, at));
      }
      // The album line has no counterpart in the bar; it rides along with the artist.
      const artist = found.find((p) => !p.ghost && p.el.matches('.hero-artist'));
      const album = section.querySelector<HTMLElement>('.hero-album');
      if (artist && album) {
        const e = artist.el.getBoundingClientRect();
        const r = album.getBoundingClientRect();
        album.style.transformOrigin = '0 0';
        const dx = artist.dx + (r.left - e.left) * (artist.sx - 1);
        const dy = artist.dy + (r.top - e.top) * (artist.sy - 1);
        found.push({ ...artist, el: album, dx, dy });
      } else if (album) fades.push(album);
      // The queue rises from below the page, as the whole sheet used to.
      const queue = section.querySelector<HTMLElement>('.queue-slot');
      if (queue) found.push({ el: queue, dx: 0, dy: at.bottom - queue.getBoundingClientRect().top, sx: 1, sy: 1, ghost: false, fade: 'none' });
      parts = found;
    }
    apply(sheet.progress, true);
    // Commit the start styles before the transition to the new progress.
    void getComputedStyle(backdrop).transform;
  }

  function apply(p: number, instant: boolean) {
    if (!section || !tint) return;
    const q = 1 - p;
    for (const { el, dx, dy, sx, sy, ghost, fade } of parts) {
      const t = ghost ? p : q;
      // How far this one is from being shown: 1 at its own end, 0 at the other.
      const r = 1 - t;
      el.style.transition = instant ? 'none' : `transform ${EASE}, ${OPACITY[fade](r)}`;
      el.style.transform = t ? `translate(${dx * t}px, ${dy * t}px) scale(${1 + (sx - 1) * t}, ${1 + (sy - 1) * t})` : '';
      if (fade === 'cross') el.style.opacity = String(Math.min(1, 2 * r));
      // At exactly halfway the sheet's is shown and the ghost isn't.
      else if (fade === 'swap') el.style.opacity = (ghost ? r > 0.5 : r >= 0.5) ? '1' : '0';
      else if (fade === 'late') el.style.opacity = String(Math.max(0, 3 * r - 2));
    }
    const morphing = parts.length > 0;
    section.style.opacity = morphing ? '' : String(p);
    for (const el of fades) {
      // Opening, fade in late; closing, fade out first.
      el.style.transition = instant ? 'none' : p ? 'opacity 120ms linear 140ms' : 'opacity 100ms linear';
      el.style.opacity = morphing ? String(Math.max(0, p * 3 - 2)) : '';
    }
    tint.style.transition = instant ? 'none' : `opacity ${EASE}`;
    tint.style.opacity = morphing ? String(q) : '0';
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
    <button class="icon-button" aria-label="Collapse" title="Collapse (Esc)" onclick={() => sheet.collapse()}>
      <Icon name="chevron-down" size={20} />
    </button>
  </header>

  <div class="body" bind:this={body}>
    <div class="now">
      <div class="art"><ArtworkWidget expands={false} /></div>
      <div class="details">
        <TrackInfo style="hero" />
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

    .queue-slot:has(:global(.queue.open)) {
      flex: none;
      height: 60vh;
    }
  }
</style>
