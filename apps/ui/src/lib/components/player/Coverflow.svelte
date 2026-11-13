<script lang="ts" module>
  /** One image in the carousel, with its caption (none when nothing says what it shows). */
  export interface Card {
    key: string;
    kicker: string;
    title: string;
  }
</script>

<script lang="ts">
  import { MediaQuery } from 'svelte/reactivity';
  import { menu } from '../../contextMenu.svelte';
  import type { MenuItem } from '../../contextMenu.svelte';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { SLOP } from '../../gestures';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';

  let {
    cards,
    index = $bindable(),
    track,
    width,
    height,
    items,
    settled,
  }: {
    cards: Card[];
    index: number;
    track: TrackRef | null | undefined;
    /** The space it fills. */
    width: number;
    height: number;
    /** The centre card's context menu. */
    items: () => MenuItem[];
    /** Neighbours, arrows and dots show only while the sheet is open or being dragged. */
    settled: boolean;
  } = $props();

  // Neighbours each side: fewer on phones, where the cover takes the width and the neighbours
  // stay behind it, and the sheet names the album instead of a caption.
  const narrow = new MediaQuery('(max-width: 700px)');
  const near = $derived(narrow.current ? 1 : 2);
  /** How much of each neighbour shows beside the centre card, as a share of its side. */
  const PEEK = $derived(narrow.current ? 0 : 0.2);
  /** The centre card's caption and the dots under the cards, with the gaps between them. */
  const BELOW = $derived(narrow.current ? 28 : 80);
  const PADDING = $derived(narrow.current ? 8 : 12);
  const side = $derived(
    Math.max(0, Math.min(height - 2 * PADDING - BELOW, (width - 2 * PADDING) / (1 + 2 * PEEK * near))),
  );
  const at = $derived(Math.min(index, cards.length - 1));
  const alone = $derived(cards.length < 2);

  /** Where card `i` sits: the centre card flat, neighbours stepping back and turned towards it. */
  function place(i: number) {
    const d = i - at;
    const k = Math.abs(d);
    const scale = 1 - 0.15 * k;
    const x = Math.sign(d) * side * (0.5 + PEEK * k - (scale * 0.5 * 0.75));
    return {
      transform: `translateX(${x}px) rotateY(${Math.sign(d) * 40}deg) scale(${scale})`,
      scale,
      hidden: k > near || (k > 0 && !settled),
      shade: Math.min(0.5, 0.22 * k),
      z: 10 - k,
    };
  }

  const go = (i: number) => (index = (i + cards.length) % cards.length);

  function onKey(e: KeyboardEvent) {
    const by = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
    if (!by) return;
    // Before the window's seek shortcuts.
    e.preventDefault();
    e.stopPropagation();
    go(at + by);
  }

  // A tap on the centre card shows the next image (on phones, where the neighbours hide behind it,
  // its left half the previous one); a drag that ends on it (a swipe, the volume gesture) or a long
  // press for the menu is not a tap.
  let down: { x: number; y: number } | null = null;
  const isTap = (e: PointerEvent) =>
    e.button === 0 && !!down && Math.hypot(e.clientX - down.x, e.clientY - down.y) < SLOP && !!(e.target as Element).closest('.tap');

  // Mouse only: a touch swipe on the artwork skips the track (`gestures.ts`).
  let swipeX: number | null = null;
  function onPointerUp(e: PointerEvent) {
    if (isTap(e)) {
      const card = (e.target as Element).closest('.tap')!.getBoundingClientRect();
      go(at + (narrow.current && e.clientX < card.left + card.width / 2 ? -1 : 1));
    }
    down = null;
    const dx = swipeX === null ? 0 : e.clientX - swipeX;
    swipeX = null;
    if (Math.abs(dx) > 40) go(at - Math.sign(dx));
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class="coverflow"
  role="region"
  aria-roledescription="carousel"
  aria-label="Album images"
  tabindex="0"
  onkeydown={onKey}
  onpointerdown={(e) => {
    down = { x: e.clientX, y: e.clientY };
    swipeX = e.pointerType === 'mouse' ? e.clientX : null;
  }}
  onpointerup={onPointerUp}
  onpointercancel={() => (swipeX = down = null)}
  oncontextmenu={() => (down = null)}
>
  <!-- Perspective without preserve-3d: cards are seen from the centre, and z-index (not depth)
       keeps the neighbours behind. -->
  <div class="stage" style:height="{side}px" style:perspective="{side * 3}px">
    {#each cards as card, i (card.key)}
      {#if Math.abs(i - at) <= near + 1}
        {@const p = place(i)}
        {@const centre = i === at}
        <!-- The slot places the card; the centre card's face is the `.frame` the sheet's open and
             close morph moves, and the other faces fade in after it (see ExpandedPlayer). -->
        <div
          class="slot"
          class:hidden={p.hidden}
          style:width="{side}px"
          style:height="{side}px"
          style:margin-left="{-side / 2}px"
          style:transform={p.transform}
          style:z-index={p.z}
        >
          <div class="card" class:frame={centre} data-move={centre ? 'cover:now' : undefined} data-enter={centre ? undefined : 'fade'} class:tap={centre && !alone} use:menu={centre ? items : () => []}>
            <!-- An empty key is the track's own cover, looked up (and held while the next loads). -->
            <Artwork {track} art={card.key || undefined} size={side} drawn={side * p.scale} />
            {#if !centre}
              <button
                class="shade"
                style:opacity={p.shade}
                tabindex="-1"
                aria-label="Show {card.title || `image ${i + 1}`}"
                onclick={() => go(i)}
              ></button>
            {/if}
          </div>
        </div>
      {/if}
    {/each}
    <!-- The sheet's morph fades `.turns` and `.below` (inline opacity); what shows inside them
         fades by class, also when the list arrives late. -->
    <!-- Phones page by swiping or tapping the cover instead. -->
    {#if !narrow.current}
    <div class="turns" data-enter="rise">
      <button class="icon-button turn previous" class:hidden={!settled || alone} aria-label="Previous image" onclick={() => go(at - 1)}>
        <Icon name="chevron-left" />
      </button>
      <button class="icon-button turn next" class:hidden={!settled || alone} aria-label="Next image" onclick={() => go(at + 1)}>
        <Icon name="chevron-right" />
      </button>
    </div>
    {/if}
  </div>
  <div class="below" data-enter="rise">
    <!-- Always two lines high, so nothing below moves as it pages. -->
    {#if !narrow.current}
      <div class="caption" class:hidden={!settled || !cards[at]?.title} style:width="{side}px" aria-live="polite">
        <span class="kicker">{cards[at]?.kicker ?? ''}</span>
        <span class="title">{cards[at]?.title ?? ''}</span>
      </div>
    {/if}
    <div class="dots" class:hidden={!settled || alone}>
    {#each cards as card, i (card.key)}
      <button
        class="dot"
        class:current={i === at}
        aria-label="Image {i + 1} of {cards.length}{card.title ? `: ${card.title}` : ''}"
        aria-current={i === at}
        onclick={() => go(i)}
        ></button>
      {/each}
    </div>
  </div>
</div>

<style>
  .coverflow {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    height: 100%;
    /* A vertical touch drag sets the volume rather than scrolling the sheet. */
    touch-action: none;
    outline: none;
  }

  .stage {
    position: relative;
    width: 100%;
  }

  .slot {
    position: absolute;
    top: 0;
    left: 50%;
    transition:
      transform 250ms ease,
      opacity 250ms ease;
  }

  /* Cards added once the sheet is open (a slow list) fade in rather than pop. */
  @starting-style {
    .slot {
      opacity: 0;
    }
  }

  .card {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    border-radius: var(--radius-lg);
    line-height: 0;
    box-shadow: 0 2px 0 rgb(0 0 0 / 0.25);
  }

  .card.tap {
    cursor: pointer;
  }

  .card :global(.art) {
    border-radius: 0;
  }

  .hidden {
    opacity: 0;
    pointer-events: none;
  }

  /* Dims the cards behind, and takes their clicks. */
  .shade {
    position: absolute;
    inset: 0;
    padding: 0;
    border: 0;
    background: #000;
    cursor: pointer;
    transition: opacity 250ms ease;
  }

  .turns {
    position: absolute;
    inset: 0;
    z-index: 20;
    pointer-events: none;
  }

  .turns .turn:not(.hidden) {
    pointer-events: auto;
  }

  .below {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .caption {
    transition: opacity 250ms ease;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    text-align: center;
    line-height: 1.25;
  }

  .kicker,
  .title {
    max-width: 100%;
    min-height: 1.25em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .kicker {
    color: var(--accent);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .title {
    color: var(--text);
    font-size: 16px;
    font-weight: 700;
  }

  .turn {
    position: absolute;
    top: 50%;
    z-index: 20;
    transition: opacity 250ms ease;
    width: 36px;
    height: 36px;
    margin-top: -18px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.5);
    color: #fff;
  }

  .turn:hover:not(:disabled) {
    background: rgb(0 0 0 / 0.7);
    color: #fff;
  }

  .previous {
    left: 4px;
  }

  .next {
    right: 4px;
  }

  .dots {
    display: flex;
    gap: 6px;
    height: 16px;
    align-items: center;
    transition: opacity 250ms ease;
  }

  .dot {
    width: 6px;
    height: 6px;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: var(--text-muted);
    cursor: pointer;
    transition:
      width 150ms ease,
      background-color 150ms ease;
  }

  .dot.current {
    width: 18px;
    background: var(--accent);
  }
</style>
