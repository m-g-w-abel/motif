<script lang="ts">
  import { MediaQuery } from 'svelte/reactivity';
  import { albumKey, groupConsecutive } from '../../albums';
  import { getApp, getViewState } from '../../context';
  import { formatTime, trackFormat, trackTitle } from '../../format';
  import type { Column, TracklistStyle } from '../../layout/model';
  import { trackListData, type ListSource } from '../../sources.svelte';
  import Icon from '../Icon.svelte';

  let {
    source,
    style = 'table',
    columns,
    maxWidth,
  }: {
    /** Defaults to whatever the music area is showing. */
    source?: ListSource;
    style?: TracklistStyle;
    columns: Column[];
    /** Centres the list at this width on wide screens. */
    maxWidth?: number;
  } = $props();

  const { engine } = getApp();
  const view = getViewState();

  const data = $derived(trackListData(engine, source ?? view.source));
  const groups = $derived(style === 'grouped' ? groupConsecutive(data.tracks) : null);

  // Phones drop secondary columns rather than squeezing everything.
  const narrow = new MediaQuery('(max-width: 700px)');
  // Grouped lists already name the artist in each album header.
  const secondary: Column[] = ['artist', 'album', 'format'];
  const shown = $derived(narrow.current ? columns.filter((c) => !secondary.includes(c)) : columns);

  /** The column that absorbs leftover width; the others have fixed widths. */
  const flexColumn = $derived<Column>(shown.includes('title') ? 'title' : shown[shown.length - 1]);

  const labels: Record<Column, string> = {
    index: '#',
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    format: 'Format',
    duration: 'Time',
  };

  const defaultWidths: Record<Column, number> = {
    index: 44,
    title: 300,
    artist: 190,
    album: 210,
    format: 72,
    duration: 64,
  };

  // Column widths are a per-device preference, remembered per view and list.
  const widthsKey = $derived(`columns.${view.layoutId}.${style}.${columns.join(',')}`);
  let widths = $state<Partial<Record<Column, number>>>({});
  $effect(() => {
    try {
      widths = JSON.parse(localStorage.getItem(widthsKey) ?? '{}');
    } catch {
      widths = {};
    }
  });

  /**
   * Widths actually applied. In a narrow panel the flexible column keeps at
   * least a quarter of the space: wide columns shrink proportionally (number, format and
   * time columns are small and keep their size).
   */
  let tableWidth = $state(0);
  /** The least room the flexible column is given. */
  const flexReserve = $derived(Math.max(100, tableWidth * 0.25));
  const applied = $derived.by(() => {
    const requested = (c: Column) => widths[c] ?? defaultWidths[c];
    const fixed = shown.filter((c) => c !== flexColumn);
    const compact = new Set<Column>(['index', 'format', 'duration']);
    const reserved = flexReserve + (data.remove ? 28 : 0);
    const compactSum = fixed.filter((c) => compact.has(c)).reduce((n, c) => n + requested(c), 0);
    const wideSum = fixed.filter((c) => !compact.has(c)).reduce((n, c) => n + requested(c), 0);
    const room = tableWidth - reserved - compactSum;
    const scale = tableWidth && wideSum > room ? Math.max(0.3, room / wideSum) : 1;
    return Object.fromEntries(
      fixed.map((c) => [c, Math.round(compact.has(c) ? requested(c) : requested(c) * scale)]),
    ) as Partial<Record<Column, number>>;
  });

  function saveWidths() {
    try {
      localStorage.setItem(widthsKey, JSON.stringify(widths));
    } catch {
      // Widths just won't persist.
    }
  }

  /**
   * Drags the boundary between column `index` and the next so it follows the
   * pointer. Only the two neighbours change: one grows as the other shrinks.
   * The flexible column has no stored width, so next to it only the fixed
   * neighbour changes and the flexible one absorbs the difference.
   */
  function startResize(e: PointerEvent, index: number) {
    e.preventDefault();
    const handle = e.currentTarget as HTMLElement;
    const left = shown[index];
    const right = shown[index + 1];
    const startX = e.clientX;
    // Start from what's on screen: if columns were shrunk to fit, adopting
    // those widths makes the boundary track the pointer exactly.
    widths = { ...widths, ...applied };
    const startLeft = widths[left] ?? defaultWidths[left];
    const startRight = widths[right] ?? defaultWidths[right];
    const min = 36;
    // How far the boundary may move without squashing a fixed neighbour.
    // The flexible column can give up space only down to its reserve.
    const flexCell = handle.closest('tr')?.querySelector<HTMLElement>(`th.${flexColumn}`);
    const flexRoom = Math.max(0, (flexCell?.offsetWidth ?? 0) - flexReserve);
    const lo = left === flexColumn ? -flexRoom : min - startLeft;
    const hi = right === flexColumn ? flexRoom : startRight - min;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const dx = Math.round(Math.min(hi, Math.max(lo, ev.clientX - startX)));
      const next = { ...widths };
      if (left !== flexColumn) next[left] = startLeft + dx;
      if (right !== flexColumn) next[right] = startRight - dx;
      widths = next;
    };
    const end = () => {
      handle.removeEventListener('pointermove', move);
      saveWidths();
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end, { once: true });
    handle.addEventListener('pointercancel', end, { once: true });
  }

  /** Resets both columns next to a boundary to their default widths. */
  function resetWidth(index: number) {
    const pair = [shown[index], shown[index + 1]];
    widths = Object.fromEntries(Object.entries(widths).filter(([c]) => !pair.includes(c as Column)));
    saveWidths();
  }

  // Selection lives in shared browse state so it survives layout switches.
  const selected = $derived(view.browse.selected);
  const select = (key: string) => (view.browse.selected = key);
  let scroller = $state<HTMLElement>();

  /** In grouped lists, the index of each track's album header row. */
  const groupStart = $derived.by(() => {
    const starts = new Map<number, number>();
    for (const g of groups ?? []) for (const i of g.indices) starts.set(i, g.indices[0]);
    return starts;
  });

  // An album picked in an album grid scrolls this list to it.
  $effect(() => {
    const key = view.browse.focusAlbum;
    if (!key || !scroller) return;
    const index = data.tracks.findIndex((t) => albumKey(t) === key);
    if (index < 0) return;
    const row = scroller.querySelector<HTMLElement>(`[data-index="${index}"]`);
    const target = (style === 'grouped' ? row?.previousElementSibling : row) as HTMLElement | null;
    if (target) {
      // Scroll this list only; scrollIntoView would also scroll every
      // scrollable ancestor, moving the whole window.
      const header = scroller.querySelector('thead')?.offsetHeight ?? 0;
      const top = target.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
      const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
      scroller.scrollTo({ top: top - header, behavior: reduceMotion ? 'auto' : 'smooth' });
    }
    view.browse.selected = data.keys[index];
  });

  function cell(column: Column, index: number): string {
    const t = data.tracks[index];
    switch (column) {
      case 'index':
        // Grouped lists number tracks within each album.
        return String(index - (groups ? (groupStart.get(index) ?? 0) : 0) + 1);
      case 'title':
        return trackTitle(t);
      case 'artist':
        return t.artist ?? '';
      case 'album':
        return t.album ?? '';
      case 'format':
        return trackFormat(t);
      case 'duration':
        return t.duration_ms ? formatTime(t.duration_ms) : '';
    }
  }

  function onKey(e: KeyboardEvent, index: number) {
    if (e.key === 'Enter') data.play(index);
    if ((e.key === 'Delete' || e.key === 'Backspace') && data.remove) data.remove(index);
  }

  const columnCount = $derived(shown.length + (data.remove ? 1 : 0));
</script>

<section class="tracklist {style}" style:--max-width={maxWidth ? `${maxWidth}px` : undefined}>
  {#if data.tracks.length === 0}
    <div class="empty">
      <p>The queue is empty.</p>
      <p class="hint">Double-click a track in the library to play it.</p>
    </div>
  {:else}
    <div class="scroll" bind:this={scroller} bind:clientWidth={tableWidth}>
      <table aria-label={data.title} style:max-width="var(--max-width, none)">
        <colgroup>
          {#each shown as column (column)}
            <col style:width={column === flexColumn ? undefined : `${applied[column]}px`} />
          {/each}
          {#if data.remove}<col style:width="28px" />{/if}
        </colgroup>
        {#if style === 'table'}
          <thead>
            <tr>
              {#each shown as column, i (column)}
                <th class={column} scope="col">
                  <span class="label">{labels[column]}</span>
                  {#if i < shown.length - 1}
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <span
                      class="resize"
                      title="Drag to resize, double-click to reset"
                      onpointerdown={(e) => startResize(e, i)}
                      ondblclick={() => resetWidth(i)}
                    ></span>
                  {/if}
                </th>
              {/each}
              {#if data.remove}<th></th>{/if}
            </tr>
          </thead>
        {/if}
        <tbody>
          {#each data.tracks as track, i (data.keys[i])}
            {#if groups && groupStart.get(i) === i}
              {@const group = groups.find((g) => g.indices[0] === i)}
              {#if group}
                <tr class="group">
                  <td colspan={columnCount}>
                    <span class="group-title">{group.artist} – {group.title}</span>
                    <span class="group-length">[{formatTime(group.durationMs)}]</span>
                  </td>
                </tr>
              {/if}
            {/if}
            <tr
              data-index={i}
              class:current={i === data.currentIndex}
              class:selected={selected === data.keys[i]}
              tabindex="0"
              aria-selected={selected === data.keys[i]}
              onclick={() => select(data.keys[i])}
              ondblclick={() => data.play(i)}
              onkeydown={(e) => onKey(e, i)}
            >
              {#each shown as column (column)}
                <td class={column}>
                  {#if column === 'index' && i === data.currentIndex}
                    <Icon name={engine.player?.status === 'playing' ? 'play' : 'pause'} size={10} />
                  {:else}
                    {cell(column, i)}
                  {/if}
                </td>
              {/each}
              {#if data.remove}
                <td class="remove-cell">
                  <button
                    class="remove"
                    aria-label="Remove {trackTitle(track)}"
                    onclick={(e) => {
                      e.stopPropagation();
                      data.remove?.(i);
                    }}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .tracklist {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  table {
    width: 100%;
    margin: 0 auto;
    table-layout: fixed;
    border-collapse: collapse;
    user-select: none;
  }

  th,
  td {
    height: var(--row-height);
    padding: 0 var(--space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    font-weight: inherit;
  }

  th:first-child,
  td:first-child {
    padding-left: var(--space-3);
  }

  thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    height: 24px;
    background: var(--bg);
    box-shadow: inset 0 -1px 0 var(--border);
    font-size: 11px;
    color: var(--text-muted);
  }

  .label {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .resize {
    position: absolute;
    top: 4px;
    right: 0;
    bottom: 4px;
    width: 7px;
    border-right: 1px solid var(--border);
    cursor: col-resize;
    touch-action: none;
  }

  .resize:hover {
    border-right-color: var(--accent);
  }

  .table tbody tr:nth-child(even) {
    background: color-mix(in srgb, var(--surface) 45%, transparent);
  }

  tbody tr:not(.group):hover {
    background: var(--hover);
  }

  tbody tr.selected,
  tbody tr.selected:hover {
    background: var(--selected);
  }

  tr:focus-visible {
    outline: 1px solid var(--accent);
    outline-offset: -1px;
  }

  tr.current td {
    color: var(--accent);
  }

  tr.current .title {
    font-weight: 600;
  }

  .index,
  .duration {
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  th.index .label,
  th.duration .label {
    padding-right: 6px;
  }

  .format {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    color: var(--text-muted);
  }

  .artist,
  .album {
    color: var(--text-muted);
  }

  /* Grouped: album headers right-aligned above their tracks. */
  tr.group td {
    height: calc(var(--row-height) + 6px);
    padding-top: 6px;
    padding-right: var(--space-3);
    border-bottom: 1px solid var(--border);
    text-align: right;
    font-size: 0.92em;
    color: var(--text-muted);
  }

  .group-length {
    margin-left: var(--space-2);
    font-variant-numeric: tabular-nums;
  }

  .remove-cell {
    padding: 0;
  }

  .remove {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0;
  }

  tr:hover .remove,
  tr.selected .remove,
  .remove:focus-visible {
    opacity: 1;
  }

  .remove:hover {
    color: var(--danger);
  }

  .empty {
    padding: var(--space-6) var(--space-4);
    text-align: center;
    color: var(--text-muted);
  }

  .empty p {
    margin: 0 0 var(--space-1);
  }

  .hint {
    font-size: 0.92em;
  }

  @media (hover: none) {
    .remove {
      opacity: 1;
    }
  }
</style>
