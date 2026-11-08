<script lang="ts" module>
  import type { Side } from '../../layout/overlay';

  /** The drag in progress, shared by every node: the dragged node and where it would land, as `At` keys. */
  const drag = $state<{ from: string | null; to: string | null; side: Side | null }>({ from: null, to: null, side: null });
</script>

<script lang="ts">
  import { tick } from 'svelte';
  import { contextMenu } from '../../contextMenu.svelte';
  import { getApp } from '../../context';
  import { WIDGETS, type LayoutNode, type Size, type Widget } from '../../layout/model';
  import { childPath, idOf, type At } from '../../layout/overlay';
  import Icon from '../Icon.svelte';
  // Every widget a layout can place. Layouts only choose and configure these.
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import Artwork from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Lyrics from '../widgets/Lyrics.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';
  import TrackList from '../widgets/TrackList.svelte';
  import Visualiser from '../widgets/Visualiser.svelte';
  import Self from './LayoutNode.svelte';

  /**
   * `at` is this node's place in the tree on screen and `path` names it (for
   * widgets' remembered state). `stack` counts the handles above this one in
   * the same corner, which push it down.
   */
  let {
    node,
    path,
    at = [],
    stack = 0,
    parent,
    siblings = 0,
  }: {
    node: LayoutNode;
    path: string;
    at?: At;
    stack?: number;
    /** The direction of the split this node is in, and how many nodes that split has. */
    parent?: 'row' | 'column';
    siblings?: number;
  } = $props();

  const { layout } = getApp();
  const key = $derived(at.join());
  let split = $state<HTMLElement>();

  const MIN = 80;

  function flex(size: Size | undefined): string {
    if (!size) return '1 1 0';
    return 'px' in size ? `0 0 ${size.px}px` : `0 0 ${size.percent}%`;
  }

  const children = $derived(node.kind === 'split' ? node.children : []);

  /** The divider before child `index` resizes the neighbour with a size (a side panel), else the one before it. */
  const targetOf = (index: number) => (!children[index - 1].size && children[index].size ? index : index - 1);

  /** Drags a divider, keeping the size in pixels or percent as the file has it. */
  function startResize(e: PointerEvent, index: number) {
    if (node.kind !== 'split' || !split) return;
    e.preventDefault();
    const divider = e.currentTarget as HTMLElement;
    const target = targetOf(index);
    const before = target === index - 1;
    const el = (before ? divider.previousElementSibling : divider.nextElementSibling) as HTMLElement;
    const row = node.direction === 'row';
    const pos = (ev: PointerEvent) => (row ? ev.clientX : ev.clientY);
    const origin = pos(e);
    const start = row ? el.offsetWidth : el.offsetHeight;
    const total = row ? split.clientWidth : split.clientHeight;
    const max = total - MIN * (children.length - 1);
    const size = children[target].size;
    const percent = size && 'percent' in size;
    const stop = new AbortController();
    divider.setPointerCapture(e.pointerId);
    divider.addEventListener(
      'pointermove',
      (ev) => {
        const px = Math.min(max, Math.max(MIN, start + (pos(ev) - origin) * (before ? 1 : -1)));
        layout.resize([...at, target], percent ? { percent: Math.round((px / total) * 1000) / 10 } : { px: Math.round(px) });
      },
      { signal: stop.signal },
    );
    divider.addEventListener('pointerup', () => stop.abort(), { signal: stop.signal });
    divider.addEventListener('pointercancel', () => stop.abort(), { signal: stop.signal });
  }

  const toAt = (key: string): At => (key ? key.split(',').map(Number) : []);

  /**
   * Drags this node's handle to the edge of any widget to move it beside
   * that widget, on the side nearest the pointer.
   */
  function startMove(e: PointerEvent) {
    e.preventDefault();
    const grip = e.currentTarget as HTMLElement;
    const inside = (k: string) => k === key || k.startsWith(`${key},`);
    const track = (ev: PointerEvent) => {
      const slot = document.elementsFromPoint(ev.clientX, ev.clientY).find((el) => el instanceof HTMLElement && el.dataset.at !== undefined);
      if (!(slot instanceof HTMLElement)) return; // Off the music area: keep the last target.
      const k = slot.dataset.at!;
      if (inside(k)) {
        drag.to = drag.side = null; // Over itself: it stays.
        return;
      }
      const r = slot.getBoundingClientRect();
      const dx = (ev.clientX - r.left) / r.width - 0.5;
      const dy = (ev.clientY - r.top) / r.height - 0.5;
      drag.to = k;
      drag.side = Math.abs(dx) > Math.abs(dy) ? (dx < 0 ? 'left' : 'right') : dy < 0 ? 'above' : 'below';
    };
    const stop = new AbortController();
    const end = (ev: PointerEvent) => {
      stop.abort();
      if (ev.type === 'pointerup' && drag.to !== null && drag.side) layout.move(at, toAt(drag.to), drag.side);
      drag.from = drag.to = drag.side = null;
    };
    drag.from = key;
    grip.setPointerCapture(e.pointerId);
    grip.addEventListener('pointermove', track, { signal: stop.signal });
    grip.addEventListener('pointerup', end, { signal: stop.signal });
    grip.addEventListener('pointercancel', end, { signal: stop.signal });
  }

  /** The arrow keys move a node past its neighbour in its split. */
  function onGripKey(e: KeyboardEvent) {
    const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[e.key];
    const i = at[at.length - 1];
    if (!step || !parent || i + step < 0 || i + step >= siblings) return;
    e.preventDefault();
    const grip = e.currentTarget as HTMLElement;
    const row = parent === 'row';
    layout.move(at, [...at.slice(0, -1), i + step], step < 0 ? (row ? 'left' : 'above') : row ? 'right' : 'below');
    // The node's element moves, which can drop focus from its handle.
    tick().then(() => grip.focus());
  }

  const nameOf = (node: LayoutNode) =>
    node.kind === 'split' ? `${node.direction === 'row' ? 'Row' : 'Column'} split` : WIDGETS[node.panel];

  const SIDES: [Side, string][] = [
    ['left', 'Add on the left…'],
    ['right', 'Add on the right…'],
    ['above', 'Add above…'],
    ['below', 'Add below…'],
  ];

  /** Asks for a side, then a widget to add there. */
  function openAdd(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    const show = (items: { label: string; run: () => void }[]) => contextMenu.show(items, box.left, box.bottom + 4, button);
    show(
      SIDES.map(([side, label]) => ({
        label,
        run: () =>
          show(
            (Object.entries(WIDGETS) as [Widget, string][]).map(([widget, label]) => ({
              label,
              run: () => layout.addWidget(at, side, widget),
            })),
          ),
      })),
    );
  }
</script>

<div
  class="slot"
  class:root={!parent}
  class:sized={node.size}
  class:hot={drag.from === key}
  class:drop={drag.to === key}
  data-side={drag.to === key ? drag.side : undefined}
  data-at={key}
  style:flex={parent && flex(node.size)}
>
  {#if node.kind === 'split'}
    <div class="split {node.direction}" bind:this={split}>
      {#each node.children as child, i (idOf(child))}
        {#if i > 0}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="divider"
            title="Drag to resize, double-click to reset"
            onpointerdown={(e) => startResize(e, i)}
            ondblclick={() => layout.resize([...at, targetOf(i)], undefined)}
          ></div>
        {/if}
        <Self
          node={child}
          path={childPath(path, i, child)}
          at={[...at, i]}
          stack={i === 0 ? stack + 1 : 0}
          parent={node.direction}
          siblings={node.children.length}
        />
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
  {:else if node.panel === 'visualiser'}
    <Visualiser />
  {:else if node.panel === 'lyrics'}
    <Lyrics />
  {/if}
  {#if layout.arranging}
    <div class="handle" style:top="{8 + stack * 36}px">
      {#if parent}
        <button
          class="grip"
          aria-label="Move {nameOf(node)}"
          title="Drag to the edge of a widget to move it there, or use the arrow keys"
          onpointerdown={startMove}
          onkeydown={onGripKey}
        >
          <Icon name="grip" size={16} />
          <span>{nameOf(node)}</span>
        </button>
      {:else}
        <span class="name">{nameOf(node)}</span>
      {/if}
      <button class="tool" aria-label="Add a widget beside {nameOf(node)}" title="Add a widget" aria-haspopup="menu" onclick={openAdd}>
        <Icon name="plus" size={14} />
      </button>
      {#if parent}
        <button class="tool" aria-label="Remove {nameOf(node)}" title="Remove" onclick={() => layout.removeWidget(at)}>
          <Icon name="close" size={14} />
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .split {
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

  /* The half of the widget the dragged one would take. */
  .slot.drop::after {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .slot[data-side='left']::after {
    right: 50%;
  }

  .slot[data-side='right']::after {
    left: 50%;
  }

  .slot[data-side='above']::after {
    bottom: 50%;
  }

  .slot[data-side='below']::after {
    top: 50%;
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

  .slot.root {
    height: 100%;
  }
</style>
