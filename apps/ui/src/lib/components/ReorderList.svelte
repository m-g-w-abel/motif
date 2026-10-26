<script lang="ts" generics="T">
  import type { Snippet } from 'svelte';
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { dropIndex, keyTarget, shift } from '../reorder';

  // Rows are dragged by their handle with pointer events (HTML5 drag-and-drop
  // is swallowed by Tauri's file drop in WebKitGTK) and move with transform.
  // Alt+Up/Down on anything in a row moves it too. `onmove` gets the new order.
  let {
    items,
    label,
    key,
    row,
    onmove,
  }: {
    items: T[];
    label: string;
    key?: (item: T) => string;
    row: Snippet<[T, number]>;
    onmove: (from: number, to: number) => void;
  } = $props();

  let list = $state<HTMLElement>();
  let drag = $state<{ from: number; to: number; dy: number; pitch: number } | null>(null);
  let mids: number[] = [];
  let startY = 0;
  let startMid = 0;

  const rows = () => [...(list?.children ?? [])] as HTMLElement[];

  function down(e: PointerEvent, from: number) {
    if (e.button !== 0) return;
    const rects = rows().map((r) => r.getBoundingClientRect());
    mids = rects.map((r) => r.top + r.height / 2);
    startY = e.clientY;
    startMid = mids[from];
    drag = { from, to: from, dy: 0, pitch: rects[1] ? rects[1].top - rects[0].top : rects[0].height };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function moved(e: PointerEvent) {
    if (!drag) return;
    const dy = e.clientY - startY;
    drag = { ...drag, dy, to: dropIndex(mids, drag.from, startMid + dy) };
  }

  function up() {
    if (!drag) return;
    const { from, to } = drag;
    drag = null;
    if (from !== to) onmove(from, to);
  }

  async function keydown(e: KeyboardEvent, from: number) {
    const to = keyTarget(e, from, items.length);
    if (to === null) return;
    e.preventDefault();
    onmove(from, to);
    await tick();
    rows()[to]?.querySelector<HTMLElement>('.handle')?.focus();
  }

  function transform(i: number) {
    if (!drag) return undefined;
    if (i === drag.from) return `translateY(${drag.dy}px)`;
    const s = shift(i, drag.from, drag.to);
    return s ? `translateY(${s * drag.pitch}px)` : undefined;
  }
</script>

<ol bind:this={list} aria-label={label}>
  {#each items as item, i (key ? key(item) : i)}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <li class:dragging={drag?.from === i} style:transform={transform(i)} onkeydown={(e) => keydown(e, i)}>
      <button
        class="handle"
        aria-label="Reorder {label} item {i + 1}: drag, or Alt+Up and Alt+Down"
        title="Drag to reorder (Alt+↑/↓)"
        onpointerdown={(e) => down(e, i)}
        onpointermove={moved}
        onpointerup={up}
        onpointercancel={up}
      >
        <Icon name="grip" size={14} />
      </button>
      {@render row(item, i)}
    </li>
  {/each}
</ol>

<style>
  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  li.dragging {
    position: relative;
    z-index: 1;
    background: var(--surface);
  }

  .handle {
    flex: none;
    display: grid;
    place-items: center;
    width: 18px;
    height: 24px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    cursor: grab;
    touch-action: none;
  }

  .dragging .handle {
    cursor: grabbing;
  }
</style>
