<script lang="ts">
  import { tick } from 'svelte';
  import { contextMenu } from '../../contextMenu.svelte';
  import { getApp } from '../../context';
  import { WIDGETS, type LayoutNode, type Size, type SplitNode, type Widget } from '../../layout/model';
  import { slots as slotsOf, type Slot } from '../../layout/overlay';
  import Icon from '../Icon.svelte';
  // Every widget a layout can place. Layouts only choose and configure these.
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import Artwork from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';
  import TrackList from '../widgets/TrackList.svelte';
  import Self from './LayoutNode.svelte';

  /** `path` keys this node's in-place edits; `depth` staggers the handles of nested splits. */
  let { node, path, depth = 0 }: { node: LayoutNode; path: string; depth?: number } = $props();

  const { layout } = getApp();
  const slots = $derived(node.kind === 'split' ? slotsOf(node, path, layout.edits) : []);
  let split = $state<HTMLElement>();
  /** The slot a dragged widget would move to. */
  let dropAt = $state<number | null>(null);
  /** The slot whose handle is being dragged. */
  let moving = $state<number | null>(null);

  const MIN = 80;

  function flex(size: Size | undefined): string {
    if (!size) return '1 1 0';
    return 'px' in size ? `0 0 ${size.px}px` : `0 0 ${size.percent}%`;
  }

  /** The divider before slot `index` resizes the neighbour with a size (a side panel), else the one before it. */
  const targetOf = (index: number): Slot => (!slots[index - 1].size && slots[index].size ? slots[index] : slots[index - 1]);

  /** Drags a divider, keeping the size in pixels or percent as the file has it. */
  function startResize(e: PointerEvent, index: number) {
    if (node.kind !== 'split' || !split) return;
    e.preventDefault();
    const divider = e.currentTarget as HTMLElement;
    const target = targetOf(index);
    const before = target === slots[index - 1];
    const el = (before ? divider.previousElementSibling : divider.nextElementSibling) as HTMLElement;
    const row = node.direction === 'row';
    const pos = (ev: PointerEvent) => (row ? ev.clientX : ev.clientY);
    const origin = pos(e);
    const start = row ? el.offsetWidth : el.offsetHeight;
    const total = row ? split.clientWidth : split.clientHeight;
    const max = total - MIN * (slots.length - 1);
    const percent = target.size && 'percent' in target.size;
    const stop = new AbortController();
    divider.setPointerCapture(e.pointerId);
    divider.addEventListener(
      'pointermove',
      (ev) => {
        const px = Math.min(max, Math.max(MIN, start + (pos(ev) - origin) * (before ? 1 : -1)));
        layout.resize(target.path, percent ? { percent: Math.round((px / total) * 1000) / 10 } : { px: Math.round(px) });
      },
      { signal: stop.signal },
    );
    divider.addEventListener('pointerup', () => stop.abort(), { signal: stop.signal });
    divider.addEventListener('pointercancel', () => stop.abort(), { signal: stop.signal });
  }

  /**
   * Drags a widget's handle to another slot of this split to move it there.
   * Pointing anywhere picks the slot nearest along the split, so a drop target always shows.
   */
  function startMove(e: PointerEvent, from: number) {
    if (!split) return;
    e.preventDefault();
    const grip = e.currentTarget as HTMLElement;
    const els = [...split.children].filter((c) => c.classList.contains('slot'));
    // Phones stack row splits, so the axis comes from the layout on screen.
    const row = getComputedStyle(split).flexDirection === 'row';
    const at = (ev: PointerEvent) => {
      const i = els.findIndex((el) => {
        const r = el.getBoundingClientRect();
        return row ? ev.clientX < r.right : ev.clientY < r.bottom;
      });
      return i < 0 ? els.length - 1 : i;
    };
    const stop = new AbortController();
    const end = (ev: PointerEvent) => {
      stop.abort();
      if (ev.type === 'pointerup' && dropAt !== null && dropAt !== from) moveTo(grip, from, dropAt);
      dropAt = moving = null;
    };
    dropAt = moving = from;
    grip.setPointerCapture(e.pointerId);
    grip.addEventListener('pointermove', (ev) => (dropAt = at(ev)), { signal: stop.signal });
    grip.addEventListener('pointerup', end, { signal: stop.signal });
    grip.addEventListener('pointercancel', end, { signal: stop.signal });
  }

  function moveTo(grip: HTMLElement, from: number, to: number) {
    if (node.kind !== 'split') return;
    layout.move(node, path, from, to);
    // The widget's element moves, which can drop focus from its handle.
    tick().then(() => grip.focus());
  }

  function onGripKey(e: KeyboardEvent, i: number) {
    const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[e.key];
    if (!step || i + step < 0 || i + step >= slots.length) return;
    e.preventDefault();
    moveTo(e.currentTarget as HTMLElement, i, i + step);
  }

  const nameOf = (node: LayoutNode) =>
    node.kind === 'split' ? `${node.direction === 'row' ? 'Row' : 'Column'} split` : WIDGETS[node.panel];

  function openAdd(e: MouseEvent, to: SplitNode, at: string) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    const items = (Object.entries(WIDGETS) as [Widget, string][]).map(([widget, label]) => ({
      label,
      run: () => layout.addWidget(to, at, widget),
    }));
    contextMenu.show(items, box.left, box.bottom + 4, button);
  }

  /** Handles of a slot sit lower the deeper its split is, so nested ones don't cover each other. */
  const top = (depth: number) => `${8 + depth * 36}px`;
</script>

{#snippet add(to: SplitNode, at: string)}
  <button
    class="tool"
    aria-label="Add a widget to {nameOf(to)}"
    title="Add a widget"
    aria-haspopup="menu"
    onclick={(e) => openAdd(e, to, at)}
  >
    <Icon name="plus" size={14} />
  </button>
{/snippet}

{#if node.kind === 'split'}
  <div class="split {node.direction}" bind:this={split}>
    {#if layout.arranging && depth === 0}
      <div class="handle" style:top={top(0)}>
        <span class="name">{nameOf(node)}</span>
        {@render add(node, path)}
      </div>
    {/if}
    {#each slots as slot, i (slot.path)}
      {#if i > 0}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="divider"
          title="Drag to resize, double-click to reset"
          onpointerdown={(e) => startResize(e, i)}
          ondblclick={() => layout.resize(targetOf(i).path, undefined)}
        ></div>
      {/if}
      <div
        class="slot"
        class:sized={slot.size}
        class:drop={dropAt === i && moving !== i}
        class:hot={moving === i}
        style:flex={flex(slot.size)}
      >
        <Self node={slot.node} path={slot.path} depth={depth + 1} />
        {#if layout.arranging}
          <div class="handle" style:top={top(depth + 1)}>
            <button
              class="grip"
              aria-label="Move {nameOf(slot.node)}"
              title="Drag to move, or use the arrow keys"
              onpointerdown={(e) => startMove(e, i)}
              onkeydown={(e) => onGripKey(e, i)}
            >
              <Icon name="grip" size={16} />
              <span>{nameOf(slot.node)}</span>
            </button>
            {#if slot.node.kind === 'split'}{@render add(slot.node, slot.path)}{/if}
            <button
              class="tool"
              aria-label="Remove {nameOf(slot.node)}"
              title="Remove"
              onclick={() => layout.removeWidget(slot.path)}
            >
              <Icon name="close" size={14} />
            </button>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{:else if node.panel === 'tracklist'}
  <TrackList albumBreaks={node.albumBreaks} headers={node.headers} columns={node.columns} maxWidth={node.maxWidth} at={path} />
{:else if node.panel === 'album-grid'}
  <AlbumGrid cardSize={node.cardSize} columns={node.columns} at={path} />
{:else if node.panel === 'artwork'}
  <Artwork />
{:else if node.panel === 'track-info'}
  <TrackInfo style={node.style} />
{:else if node.panel === 'controls'}
  <div class="controls"><Controls /></div>
{:else if node.panel === 'queue'}
  <Queue collapsible={node.collapsible} />
{/if}

<style>
  .split {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }

  /* Panels are separated by 1px rules, with a wider strip to grab. */
  .divider {
    position: relative;
    z-index: 1;
    flex: 0 0 1px;
    background: var(--border);
    touch-action: none;
  }

  .divider::before {
    content: '';
    position: absolute;
    inset: -4px 0;
  }

  .split.row > .divider {
    cursor: col-resize;
  }

  .split.row > .divider::before {
    inset: 0 -4px;
  }

  .split.column > .divider {
    cursor: row-resize;
  }

  .divider:hover {
    background: var(--accent);
  }

  @media (pointer: coarse) {
    .divider::before {
      inset: -10px 0;
    }

    .split.row > .divider::before {
      inset: 0 -10px;
    }
  }

  /* Drawn over the widget, which would otherwise paint over an outline on the slot. */
  .slot.drop::after,
  .slot.hot::after,
  .slot:has(> .handle :is(:hover, :focus-visible))::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 3;
    border: 2px dashed var(--accent);
    pointer-events: none;
  }

  .slot.drop::after {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .handle {
    position: absolute;
    left: 8px;
    z-index: 4;
    display: flex;
    align-items: center;
    height: 28px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-md);
    background: var(--bg);
    color: var(--accent);
    font-size: 12px;
    white-space: nowrap;
  }

  .handle > * {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 100%;
    padding: 0 6px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
  }

  .handle > * + * {
    border-left: 1px solid var(--accent);
  }

  .handle .name {
    padding-left: 8px;
  }

  .grip {
    cursor: grab;
    touch-action: none;
  }

  .split.row {
    flex-direction: row;
  }

  .split.column {
    flex-direction: column;
  }

  .controls {
    display: grid;
    place-items: center;
    height: 100%;
    padding: var(--space-3);
  }

  .slot {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
  }

  /*
   * Phones stack row splits. Fixed-size slots (side lists) shrink to their
   * content; flexible ones get most of a screen.
   */
  @media (max-width: 700px) {
    .split.row {
      flex-direction: column;
      height: auto;
      min-height: 100%;
    }

    .split.row > .slot {
      flex: none !important;
      min-height: 70vh;
    }

    .split.row > .slot.sized {
      min-height: 0;
    }

    /* Stacked slots take their height from their content, so there's nothing to drag. */
    .split.row > .divider {
      pointer-events: none;
    }
  }
</style>
