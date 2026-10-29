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
    /** Neighbours, arrows and dots show only once the sheet has finished opening. */
    settled: boolean;
  } = $props();

  // Neighbours each side: fewer on phones, where the cover takes the width.
  const narrow = new MediaQuery('(max-width: 700px)');
  const near = $derived(narrow.current ? 1 : 2);
  /** How much of each neighbour shows beside the centre card, as a share of its side. */
  const PEEK = 0.2;
  /** The centre card's caption and the dots under the cards, with the gaps between them. */
  const BELOW = 80;
  const PADDING = 12;
  const side = $derived(
    Math.max(0, Math.min(height - 2 * PADDING - BELOW, (width - 2 * PADDING) / (1 + 2 * PEEK * near))),
  );
  const at = $derived(Math.min(index, cards.length - 1));

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

  let swipeX: number | null = null;
  function onPointerUp(e: PointerEvent) {
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
  onpointerdown={(e) => (swipeX = e.clientX)}
  onpointerup={onPointerUp}
  onpointercancel={() => (swipeX = null)}
>
  <!-- Perspective without preserve-3d: cards are seen from the centre, and z-index (not depth)
       keeps the neighbours behind. -->
  <div class="stage" style:height="{side}px" style:perspective="{side * 3}px">
    {#each cards as card, i (card.key)}
      {#if Math.abs(i - at) <= near + 1}
        {@const p = place(i)}
        {@const centre = i === at}
        <!-- The centre card is the `.frame` the sheet's open and close morph moves. -->
        <div
          class="card"
          class:frame={centre}
          class:hidden={p.hidden}
          style:width="{side}px"
          style:height="{side}px"
          style:margin-left="{-side / 2}px"
          style:transform={p.transform}
          style:z-index={p.z}
          use:menu={centre ? items : () => []}
        >
          <Artwork {track} art={card.key} size={side} drawn={side * p.scale} />
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
      {/if}
    {/each}
    {#if settled}
      <button class="icon-button turn previous" aria-label="Previous image" onclick={() => go(at - 1)}>
        <Icon name="chevron-left" />
      </button>
      <button class="icon-button turn next" aria-label="Next image" onclick={() => go(at + 1)}>
        <Icon name="chevron-right" />
      </button>
    {/if}
  </div>
  <!-- Always two lines high, so nothing below moves as it pages. -->
  <div class="caption" style:width="{side}px" aria-live="polite">
    <span class="kicker">{cards[at]?.kicker ?? ''}</span>
    <span class="title">{cards[at]?.title ?? ''}</span>
  </div>
  <div class="dots" class:hidden={!settled}>
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

<style>
  .coverflow {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    height: 100%;
    touch-action: pan-y;
    outline: none;
  }

  .stage {
    position: relative;
    width: 100%;
  }

  .card {
    position: absolute;
    top: 0;
    left: 50%;
    overflow: hidden;
    border-radius: var(--radius-lg);
    line-height: 0;
    box-shadow: 0 2px 0 rgb(0 0 0 / 0.25);
    transition:
      transform 250ms ease,
      opacity 250ms ease;
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

  .caption {
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
