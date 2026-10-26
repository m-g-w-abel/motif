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

  /** Bar/floating element → its counterpart here; `true` scales evenly (text and buttons). */
  const MORPH: [string, string, boolean][] = [
    ['.art', '.frame', false],
    ['.label', '.info', true],
    ['.transport', '.controls .transport', true],
    ['.seek', '.controls .seek', false],
  ];
  /** Everything else fades in once the background has mostly grown. */
  const FADE = 'header, .queue-slot, .controls .side, .controls .seek .time';
  const EASE = '260ms cubic-bezier(0.2, 0.8, 0.2, 1)';

  type Part = { el: HTMLElement; dx: number; dy: number; sx: number; sy: number };
  // Plain, not state: measured once per gesture, read by the effect below.
  let parts: Part[] = [];
  let fades: HTMLElement[] = [];

  function shown(root: Element, selector: string) {
    return [...root.querySelectorAll<HTMLElement>(selector)].find((e) => e.offsetWidth > 0);
  }

  /** FLIP: the transform that puts `el` (at rest) over `from`. */
  function part(el: HTMLElement, from: DOMRect, even: boolean): Part | undefined {
    const to = el.getBoundingClientRect();
    if (!to.width || !to.height) return;
    let sx = from.width / to.width;
    let sy = from.height / to.height;
    if (even) sx = sy = Math.min(sx, sy);
    return { el, dx: from.left - to.left, dy: from.top - to.top, sx, sy };
  }

  /**
   * Measures where each part starts (in the bar or floating player) and ends
   * (here, untransformed), then pins them at the current progress without
   * easing, so the next change animates from there. Without a player to grow
   * out of, or with reduced motion, the whole sheet fades instead.
   */
  function measure() {
    if (!section || !backdrop) return;
    for (const { el } of parts) {
      el.style.transition = 'none';
      el.style.transform = '';
    }
    parts = [];
    fades = [...section.querySelectorAll<HTMLElement>(FADE)];
    const origin = sheet.origin;
    if (origin && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const found = [part(backdrop, origin.getBoundingClientRect(), false)];
      for (const [from, to, even] of MORPH) {
        const a = shown(origin, from);
        const b = shown(section, to);
        if (a && b) found.push(part(b, a.getBoundingClientRect(), even));
      }
      parts = found.filter((p) => !!p);
    }
    apply(sheet.progress, true);
    // Commit the start styles before the transition to the new progress.
    void getComputedStyle(backdrop).transform;
  }

  function apply(p: number, instant: boolean) {
    if (!section || !tint) return;
    const q = 1 - p;
    const transition = instant ? 'none' : `transform ${EASE}`;
    for (const { el, dx, dy, sx, sy } of parts) {
      el.style.transition = transition;
      el.style.transform = q ? `translate(${dx * q}px, ${dy * q}px) scale(${1 + (sx - 1) * q}, ${1 + (sy - 1) * q})` : '';
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

  .now :global(:is(.frame, .info, .transport, .seek)) {
    transform-origin: 0 0;
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
