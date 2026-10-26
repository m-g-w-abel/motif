<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { MediaQuery } from 'svelte/reactivity';
  import { getApp, getViewState } from '../../context';
  import { addFolder } from '../../folders';
  import { formatTime, trackFormat, trackTitle } from '../../format';
  import type { Column, TracklistStyle } from '../../layout/model';
  import type { TrackGroup } from '../../protocol/TrackGroup';
  import { trackListData, type ListSource } from '../../sources.svelte';
  import { DEFAULT_WIDTHS, fitColumns, flexReserve, visibleRange } from '../../table';
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

  const { engine, library, settings } = getApp();
  const view = getViewState();

  const kind = $derived(source ?? view.source);
  const data = $derived(trackListData(engine, library, kind, kind === 'library' ? view.browse.search || null : null));
  $effect(() => data.sync?.());
  const groups = $derived(style === 'grouped' ? data.groups : null);
  const currentKey = $derived(data.currentKey);

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

  /** Widths actually applied: narrow panels shrink columns to fit. */
  let tableWidth = $state(0);
  const reserve = $derived(flexReserve(tableWidth));
  const applied = $derived(fitColumns(widths, shown, flexColumn, tableWidth, reserve + (data.remove ? 28 : 0)));

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
    const startLeft = widths[left] ?? DEFAULT_WIDTHS[left];
    const startRight = widths[right] ?? DEFAULT_WIDTHS[right];
    const min = 36;
    // How far the boundary may move without squashing a fixed neighbour.
    // The flexible column can give up space only down to its reserve.
    const flexCell = handle.closest('tr')?.querySelector<HTMLElement>(`th.${flexColumn}`);
    const flexRoom = Math.max(0, (flexCell?.offsetWidth ?? 0) - reserve);
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

  /** In grouped lists, where the album containing track `index` starts. */
  function groupStart(index: number): number {
    if (!groups?.length) return 0;
    let lo = 0;
    let hi = groups.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (groups[mid].start <= index) lo = mid;
      else hi = mid - 1;
    }
    return groups[lo].start <= index ? groups[lo].start : 0;
  }

  // Only the rows in view are in the DOM, so long libraries stay fast. Row
  // heights come from the CSS (density) and are measured once rendered.
  type Row = { group: TrackGroup } | { index: number };
  let rowHeight = $state(26);
  let groupHeight = $state(32);
  let scrollTop = $state(0);
  let viewHeight = $state(0);
  let tbody = $state<HTMLElement>();
  const rows = $derived.by(() => {
    const list: Row[] = [];
    const tops: number[] = [];
    let y = 0;
    const push = (row: Row, height: number) => {
      list.push(row);
      tops.push(y);
      y += height;
    };
    // Groups arrive separately from rows, so tolerate them not matching yet.
    const count = data.count;
    let next = 0;
    for (const group of groups ?? []) {
      if (group.start < next || group.start >= count) continue;
      for (; next < group.start; next++) push({ index: next }, rowHeight);
      push({ group }, groupHeight);
      for (; next < Math.min(group.start + group.count, count); next++) push({ index: next }, rowHeight);
    }
    for (; next < count; next++) push({ index: next }, rowHeight);
    return { list, tops, height: y };
  });
  const OVERSCAN = 400;
  const range = $derived(visibleRange(rows.tops, scrollTop - OVERSCAN, scrollTop + viewHeight + OVERSCAN));
  const visible = $derived(rows.list.slice(range[0], range[1]));
  const padTop = $derived(rows.tops[range[0]] ?? 0);
  const padBottom = $derived(rows.height - (rows.tops[range[1]] ?? rows.height));
  // Rows inserted above the first visible one (a scan) must not push it away.
  // Re-pinned on scroll only: a refetch changes `rows` before the shift is reported.
  let anchorOffset = 0;
  $effect(() => {
    const top = scrollTop;
    untrack(() => {
      let at = range[0];
      while (at < rows.list.length && !('index' in rows.list[at] && rows.tops[at] >= top)) at++;
      if (at >= rows.list.length) return;
      anchorOffset = top - rows.tops[at];
      data.anchor?.((rows.list[at] as { index: number }).index, async (index) => {
        await tick();
        const to = rows.list.findIndex((row) => 'index' in row && row.index === index);
        if (to >= 0 && scroller) scroller.scrollTop = rows.tops[to] + anchorOffset;
      });
    });
  });
  const rowKey = (row: Row) => ('group' in row ? `\0group${row.group.start}` : data.key(row.index));

  $effect(() => {
    void visible;
    void settings.current.density;
    // After the frame, once the density's CSS has applied everywhere.
    const frame = requestAnimationFrame(() => {
      const track = tbody?.querySelector<HTMLElement>('tr.track');
      const group = tbody?.querySelector<HTMLElement>('tr.group');
      if (track) rowHeight = track.offsetHeight;
      if (group) groupHeight = group.offsetHeight;
    });
    return () => cancelAnimationFrame(frame);
  });

  // An album picked in an album grid scrolls this list to it. Only the focus
  // triggers this; list updates (e.g. a track change) must not move the user.
  $effect(() => {
    const key = view.browse.focusAlbum;
    if (!key || !scroller) return;
    untrack(() => {
      const index = data.groups.find((g) => g.key === key)?.start ?? -1;
      if (index < 0 || !scroller) return;
      // Grouped lists scroll to the album header just above its first track.
      const at = rows.list.findIndex((row) => 'index' in row && row.index === index);
      const target = style === 'grouped' ? at - 1 : at;
      // Scroll this list only; scrollIntoView would also scroll every
      // scrollable ancestor, moving the whole window.
      const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
      scroller.scrollTo({ top: rows.tops[target], behavior: reduceMotion ? 'auto' : 'smooth' });
      view.browse.selected = data.key(index);
    });
  });

  function cell(column: Column, index: number): string {
    const t = data.track(index);
    if (!t) return '';
    switch (column) {
      case 'index':
        // Grouped lists number tracks within each album.
        return String(index - groupStart(index) + 1);
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
  {#if data.count === 0}
    <div class="empty">
      <p>{data.empty.message}</p>
      {#if data.empty.hint}<p class="hint">{data.empty.hint}</p>{/if}
      {#if data.empty.addFolder}
        <button class="button primary" onclick={() => addFolder(engine)}>Add folder…</button>
      {/if}
    </div>
  {:else}
    <div
      class="scroll"
      bind:this={scroller}
      bind:clientWidth={tableWidth}
      bind:clientHeight={viewHeight}
      onscroll={() => (scrollTop = scroller?.scrollTop ?? 0)}
    >
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
        <tbody bind:this={tbody}>
          {#if padTop}
            <tr class="spacer" aria-hidden="true"><td colspan={columnCount} style:height="{padTop}px"></td></tr>
          {/if}
          {#each visible as row (rowKey(row))}
            {#if 'group' in row}
              <tr class="group">
                <td colspan={columnCount}>
                  <span class="group-title">{row.group.artist || 'Unknown artist'} – {row.group.title || 'Unknown album'}</span>
                  <span class="group-length">[{formatTime(row.group.duration_ms)}]</span>
                </td>
              </tr>
            {:else}
              {@const i = row.index}
              {@const track = data.track(i)}
              {@const key = data.key(i)}
              {@const error = data.error?.(i)}
              <tr
                class="track"
                class:alt={i % 2 === 1}
                class:current={key === currentKey}
                class:unplayable={!!error}
                class:selected={selected === key}
                tabindex="0"
                aria-selected={selected === key}
                onclick={() => select(key)}
                ondblclick={() => data.play(i)}
                onkeydown={(e) => onKey(e, i)}
              >
                {#each shown as column (column)}
                  <td class={column}>
                    {#if column === 'index' && key === currentKey}
                      <Icon name={engine.player?.status === 'playing' ? 'play' : 'pause'} size={10} />
                    {:else}
                      {cell(column, i)}
                      {#if column === 'title' && error}<span class="reason">{error}</span>{/if}
                    {/if}
                  </td>
                {/each}
                {#if data.remove}
                  <td class="remove-cell">
                    <button
                      class="remove"
                      aria-label="Remove {track ? trackTitle(track) : 'track'}"
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
            {/if}
          {/each}
          {#if padBottom}
            <tr class="spacer" aria-hidden="true"><td colspan={columnCount} style:height="{padBottom}px"></td></tr>
          {/if}
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

  .table tbody tr.alt {
    background: color-mix(in srgb, var(--surface) 45%, transparent);
  }

  tbody tr.track:hover {
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

  tr.unplayable td {
    color: var(--text-muted);
  }

  .reason {
    margin-left: var(--space-2);
    font-size: 0.88em;
    color: var(--danger);
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

  .remove-cell,
  .spacer td {
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

  .empty .button {
    margin-top: var(--space-3);
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
