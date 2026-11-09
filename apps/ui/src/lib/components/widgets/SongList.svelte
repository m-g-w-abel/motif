<script lang="ts">
  import { tick, untrack, type Snippet } from 'svelte';
  import { getApp, getViewState } from '../../context';
  import { addFolder } from '../../folders';
  import { trackTitle } from '../../format';
  import { gesture, mapped } from '../../gestures';
  import { navTarget, scrollInto } from '../../keynav';
  import { menu } from '../../contextMenu.svelte';
  import { playTracks, queueTracks, trackMenu } from '../../menus';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { ordered, span, toggle, type ItemAt } from '../../selection';
  import { songInfo, songNumber } from '../../songs';
  import type { SongListData } from '../../sources.svelte';
  import type { ViewMode } from '../../viewOptions';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';
  import Scrollbar from '../Scrollbar.svelte';

  // A song view's songs (All Songs, the recents, an album or playlist page): a
  // numbered list, a list with covers, or a cover grid. Only the rows in view
  // are drawn. A press plays from that song; Select mode picks songs instead.
  let {
    data,
    mode = 'list',
    columns: wanted,
    titles = true,
    artists = true,
    inAlbum = false,
    cardSize = 150,
    selecting = $bindable(false),
    shownColumns = $bindable(),
    header,
    openAlbum,
  }: {
    data: SongListData;
    mode?: ViewMode;
    /** Grid columns; fits as many as the width allows if absent. */
    columns?: number;
    titles?: boolean;
    artists?: boolean;
    /** An album's page: rows show track numbers and "disc · length · format", not the artist. */
    inAlbum?: boolean;
    cardSize?: number;
    /** Select mode: presses pick songs, and a bar acts on them. */
    selecting?: boolean;
    /** The columns the grid shows now, for the view's options. */
    shownColumns?: number;
    /** Scrolls with the songs, above them (an album page's cover and toolbar). */
    header?: Snippet;
    openAlbum?: (key: string, title: string) => void;
  } = $props();

  const { engine, settings, playlists } = getApp();
  const view = getViewState();
  $effect(() => data.sync?.());

  const grid = $derived(mode === 'grid');
  const GAP = $derived(grid ? 16 : 0);
  let width = $state(0);
  let viewHeight = $state(0);
  let scrollTop = $state(0);
  let cardHeight = $state(0);
  /** Where the songs start in the scroller, below the header. */
  let listTop = $state(0);
  let scroller = $state<HTMLElement>();
  let list = $state<HTMLElement>();

  const fit = (card: number) => Math.max(1, Math.floor((width - GAP) / (card + GAP)));
  const columns = $derived(!grid ? 1 : wanted ? Math.min(wanted, fit(80)) : fit(cardSize));
  $effect(() => {
    shownColumns = columns;
  });
  const rowHeight = $derived((cardHeight || (grid ? cardSize + 48 : 52)) + GAP);
  const count = $derived(data.count);
  const rowCount = $derived(Math.ceil(count / columns));
  const firstRow = $derived(Math.max(0, Math.floor((scrollTop - listTop) / rowHeight) - 2));
  const lastRow = $derived(Math.min(rowCount, Math.ceil((scrollTop - listTop + viewHeight) / rowHeight) + 2));
  const visible = $derived.by(() => {
    const indices = [];
    for (let i = firstRow * columns; i < Math.min(count, lastRow * columns); i++) indices.push(i);
    return indices;
  });

  // A new order or search starts at the top.
  let order: object | undefined;
  $effect(() => {
    const next = data.order;
    untrack(() => {
      if (order && next !== order && scroller) scroller.scrollTop = 0;
      order = next;
    });
  });

  $effect(() => {
    void visible;
    void width;
    void mode;
    void titles;
    void artists;
    void settings.current.density;
    const frame = requestAnimationFrame(() => {
      cardHeight = list?.querySelector('li')?.offsetHeight ?? 0;
      listTop = list?.offsetTop ?? 0;
    });
    return () => cancelAnimationFrame(frame);
  });

  // Picked songs live in the view's browse state, as in the track list.
  const picked = $derived(view.browse.picked);
  const itemAt: ItemAt<TrackRef> = (i) => {
    const track = data.track(i);
    return track && [data.key(i), track];
  };
  $effect(() => {
    if (!selecting) untrack(() => view.browse.picked.size && (view.browse.picked = new Map()));
  });
  async function selectAll() {
    await data.load?.(0, count - 1);
    view.browse.picked = span(itemAt, 0, count - 1);
  }
  const chosen = () => ordered(picked);

  function press(e: PointerEvent, i: number) {
    if (selecting) return;
    const queue = (next = false) => {
      const track = data.track(i);
      if (track) queueTracks(engine, [track], next);
    };
    gesture(e, mapped(settings.current.gestures, 'row'), { queue: () => queue(), 'play-next': () => queue(true) });
  }
  function activate(i: number) {
    if (selecting) view.browse.picked = toggle(picked, itemAt, i);
    else if (engine.can('playback')) data.play(i);
  }

  /** Plays every song, in order or shuffled: the engine's shuffle, its first song a random one. */
  export async function playEverything(shuffle = false) {
    if (!count) return;
    try {
      await engine.send({ type: 'set_shuffle', on: shuffle });
      data.play(shuffle ? Math.floor(Math.random() * count) : 0);
    } catch (e) {
      engine.error = (e as Error).message;
    }
  }

  // Roving focus: one row is in the tab order, the first drawn one standing in when it scrolls away.
  let cursor = $state(0);
  const tabStop = $derived(visible.includes(cursor) ? cursor : (visible[0] ?? -1));
  async function onKey(e: KeyboardEvent, i: number) {
    if (e.key === 'Escape' && selecting) {
      e.stopPropagation(); // Select mode goes first, then the view
      selecting = false;
      return;
    }
    const page = Math.max(1, Math.floor(viewHeight / rowHeight));
    const to = navTarget(e, i, count, columns, page);
    if (to === null || !scroller) return;
    e.preventDefault();
    cursor = to;
    const top = listTop + Math.floor(to / columns) * rowHeight;
    scroller.scrollTop = scrollInto(scroller.scrollTop, scroller.clientHeight, top, top + rowHeight);
    scrollTop = scroller.scrollTop;
    await tick();
    const row = () => list?.querySelector<HTMLElement>(`[data-index="${to}"]`);
    if (!row()) await new Promise(requestAnimationFrame);
    row()?.focus({ preventScroll: true });
  }

  function addToPlaylist(e: MouseEvent) {
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    playlists.pick(chosen(), box.left, box.top - 8, e.currentTarget as HTMLElement);
  }
</script>

<section class="song-list" class:selecting>
  <div
    class="scroll"
    bind:this={scroller}
    bind:clientHeight={viewHeight}
    onscroll={(e) => (scrollTop = (e.currentTarget as HTMLElement).scrollTop)}
  >
    {@render header?.()}
    <ul
      class:rows={!grid}
      class:covers={mode === 'covers'}
      aria-label={data.title}
      bind:this={list}
      bind:clientWidth={width}
      style:--card="{cardSize}px"
      style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
      style:padding-top="{GAP + firstRow * rowHeight}px"
      style:padding-bottom="{GAP + (rowCount - lastRow) * rowHeight}px"
      style:gap="{GAP}px"
    >
      {#each visible as i (data.key(i))}
        {@const track = data.track(i)}
        {@const details = data.details?.(i)}
        {@const key = data.key(i)}
        {@const error = data.error?.(i)}
        <li>
          {#if track}
            <button
              class="song"
              class:playing={track.id === data.currentKey}
              class:picked={picked.has(key)}
              class:unplayable={!!error}
              data-index={i}
              tabindex={i === tabStop ? 0 : -1}
              aria-pressed={selecting ? picked.has(key) : undefined}
              title={error ?? undefined}
              onfocus={() => (cursor = i)}
              onkeydown={(e) => onKey(e, i)}
              onpointerdown={(e) => press(e, i)}
              onclick={() => activate(i)}
              use:menu={() => trackMenu(engine, playlists, data, view, i, openAlbum)}
            >
              {#if mode !== 'list'}
                <Artwork {track} art={details ? details.art : undefined} title={track.album ?? ''} size={grid ? undefined : 40} />
              {:else if selecting}
                <span class="check" aria-hidden="true">{#if picked.has(key)}<Icon name="check" size={14} />{/if}</span>
              {:else}
                <span class="number">{songNumber(i, details, inAlbum)}</span>
              {/if}
              {#if grid}
                {#if titles}<span class="title">{trackTitle(track)}</span>{/if}
                {#if artists}<span class="artist">{track.artist || 'Unknown artist'}</span>{/if}
              {:else}
                <span class="text">
                  <span class="title">{trackTitle(track)}</span>
                  {#if !inAlbum}<span class="artist">{track.artist || 'Unknown artist'}</span>{/if}
                </span>
                <span class="info">{error ?? songInfo(track, details, inAlbum)}</span>
              {/if}
            </button>
          {:else}
            <div class="song loading" aria-hidden="true">
              {#if mode !== 'list'}<Artwork track={undefined} art={null} size={grid ? undefined : 40} />{/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if count === 0}
      <div class="empty">
        <p>{data.empty.message}</p>
        {#if data.empty.hint}<p class="hint">{data.empty.hint}</p>{/if}
        {#if data.empty.addFolder && engine.can('library')}
          <button class="button primary" onclick={() => addFolder(engine)}>Add folder…</button>
        {/if}
      </div>
    {/if}
  </div>
  <Scrollbar target={scroller} />
  {#if selecting}
    <div class="select-bar" role="toolbar" aria-label="Selected songs">
      <span class="picked-count" aria-live="polite">{picked.size} selected</span>
      <button onclick={() => (picked.size === count ? (view.browse.picked = new Map()) : selectAll())}>
        {picked.size === count && count ? 'None' : 'All'}
      </button>
      {#if engine.can('playback')}
        <button disabled={!picked.size} onclick={() => playTracks(engine, chosen())}>Play</button>
      {/if}
      {#if engine.can('queue')}
        <button disabled={!picked.size} onclick={() => queueTracks(engine, chosen())}>Add to queue</button>
      {/if}
      {#if playlists.canAdd(chosen())}
        <button aria-haspopup="menu" onclick={addToPlaylist}>Add to playlist…</button>
      {/if}
      <span class="grow"></span>
      <button class="done" onclick={() => (selecting = false)}>Done</button>
    </div>
  {/if}
</section>

<style>
  .song-list {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  ul {
    display: grid;
    margin: 0;
    padding: 0 16px;
    list-style: none;
  }

  .rows {
    padding-left: 0;
    padding-right: 0;
  }

  .song {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 0 0 var(--space-2);
    /* A border, not a box-shadow: blurred shadows are slow to repaint when WebKitGTK draws on the CPU. */
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    text-align: left;
    cursor: pointer;
    outline: 2px solid transparent;
    outline-offset: 1px;
  }

  .song :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  .rows .song {
    flex-direction: row;
    align-items: center;
    gap: var(--space-3);
    height: 52px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: 0;
    background: none;
    outline-offset: -2px;
  }

  .covers .song {
    height: 56px;
  }

  .rows .song :global(.art) {
    width: 40px;
    border-radius: var(--radius-sm);
    flex: none;
  }

  .rows .song:hover {
    background: var(--hover);
  }

  .song:focus-visible {
    outline-color: var(--text);
  }

  .song.picked,
  .rows .song.picked:hover {
    background: var(--selected);
  }

  .song.playing .title,
  .song.playing .number {
    color: var(--accent);
    font-weight: 600;
  }

  .song.unplayable .title {
    color: var(--text-muted);
  }

  .number,
  .check {
    flex: none;
    min-width: 2ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .check {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: 1px solid var(--border);
    border-radius: 50%;
    color: var(--accent);
  }

  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title,
  .artist,
  .info {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    margin-top: var(--space-2);
    padding: 0 var(--space-2);
    font-weight: 500;
  }

  .artist {
    padding: 0 var(--space-2);
    font-size: 0.88em;
    color: var(--text-muted);
  }

  .rows .title,
  .rows .artist {
    margin: 0;
    padding: 0;
  }

  .info {
    flex: none;
    max-width: 45%;
    font-size: 0.88em;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .loading {
    min-height: calc(var(--card) + 40px);
    cursor: default;
  }

  .rows .loading {
    min-height: 0;
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

  .select-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-top: 1px solid var(--border);
    background: var(--surface);
    flex: none;
  }

  .select-bar button {
    height: 32px;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: none;
    color: var(--text);
    cursor: pointer;
  }

  .select-bar button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .select-bar .done {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
  }

  .picked-count {
    font-size: 0.92em;
    font-variant-numeric: tabular-nums;
  }

  .grow {
    flex: 1;
  }
</style>
