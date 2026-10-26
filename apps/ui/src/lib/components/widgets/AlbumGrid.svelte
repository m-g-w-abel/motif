<script lang="ts">
  import { untrack } from 'svelte';
  import { groupConsecutive } from '../../albums';
  import { getApp, getViewState } from '../../context';
  import { demoLibrary, isDemo } from '../../demo';
  import { addFolder } from '../../folders';
  import { formatTime } from '../../format';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { viewSort } from '../../sort.svelte';
  import Artwork from '../player/Artwork.svelte';

  let { cardSize, columns: layoutColumns }: { cardSize: number; columns?: number } = $props();

  const { engine, library } = getApp();
  const view = getViewState();

  interface Card {
    key: string;
    title: string;
    artist: string;
    tracks: number;
    duration_ms: number;
    art?: string | null;
    /** For the placeholder artwork when there is no real artwork. */
    track?: TrackRef;
  }

  // Development's demo library, grouped into albums.
  const demoAlbums: Card[] = isDemo
    ? groupConsecutive(demoLibrary).map((g) => ({ ...g, tracks: g.count, track: demoLibrary[g.start] }))
    : [];
  const sort = $derived(viewSort(view));
  const albums = $derived(isDemo ? null : library.albums(view.browse.search || null, sort));
  $effect(() => albums?.sync());
  const count = $derived(albums ? albums.total : demoAlbums.length);
  const card = (i: number): Card | undefined => (albums ? albums.get(i) : demoAlbums[i]);

  function play(album: Card) {
    if (album.track) {
      const start = demoLibrary.indexOf(album.track);
      engine.run({ type: 'set_queue', tracks: demoLibrary.slice(start, start + album.tracks), start: 0 });
    } else {
      engine.query({ type: 'play_album', album: album.key, sort }).catch((e: Error) => (engine.error = e.message));
    }
  }

  /** The playing track's album, keyed the way the library keys albums. */
  const playingKey = $derived.by(() => {
    const track = engine.current?.track;
    if (!track) return null;
    const artist = library.details(track)?.album_artist ?? track.artist ?? '';
    return `${artist}\u001f${track.album ?? ''}`;
  });

  // Only the rows of cards in view are in the DOM. Columns follow the CSS
  // grid's auto-fill rule; the card height is measured once rendered.
  const GAP = 16;
  let width = $state(0);
  let viewHeight = $state(0);
  let scrollTop = $state(0);
  let cardHeight = $state(0);
  let list = $state<HTMLElement>();
  // `width` includes the 16px side padding.
  const fit = (card: number) => Math.max(1, Math.floor((width - 2 * GAP + GAP) / (card + GAP)));
  const MIN_CARD = 80;
  const maxColumns = $derived(fit(MIN_CARD));
  const autoColumns = $derived(fit(cardSize));

  // The user's choice (Ctrl+scroll, −/+) beats the layout's `columns`, which
  // beats the automatic count. Remembered per layout on this device.
  const colsKey = $derived(`album-columns.${view.layoutId}`);
  let chosen = $state<number>();
  $effect(() => {
    try {
      const n = Number(localStorage.getItem(colsKey));
      chosen = n >= 1 ? n : undefined;
    } catch {
      chosen = undefined;
    }
  });
  const wanted = $derived(chosen ?? layoutColumns);
  const columns = $derived(wanted ? Math.min(wanted, maxColumns) : autoColumns);
  function choose(n: number | undefined) {
    chosen = n === undefined ? undefined : Math.max(1, Math.min(n, maxColumns));
    try {
      if (chosen === undefined) localStorage.removeItem(colsKey);
      else localStorage.setItem(colsKey, String(chosen));
    } catch {}
  }
  // Wheel up = bigger cards = fewer columns.
  function wheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    choose(columns + (e.deltaY > 0 ? 1 : -1));
  }
  $effect(() => {
    scroller?.addEventListener('wheel', wheel, { passive: false });
    return () => scroller?.removeEventListener('wheel', wheel);
  });
  const rowHeight = $derived((cardHeight || cardSize + 48) + GAP);
  const rowCount = $derived(Math.ceil(count / columns));
  const firstRow = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - 1));
  const lastRow = $derived(Math.min(rowCount, Math.ceil((scrollTop + viewHeight) / rowHeight) + 1));
  const visible = $derived.by(() => {
    const indices = [];
    for (let i = firstRow * columns; i < Math.min(count, lastRow * columns); i++) indices.push(i);
    return indices;
  });

  // Cards inserted above the first visible row (a scan) must not push it away.
  let scroller = $state<HTMLElement>();
  let anchorOffset = 0;
  $effect(() => {
    const top = scrollTop;
    untrack(() => {
      const row = Math.ceil(top / rowHeight);
      anchorOffset = top - row * rowHeight;
      albums?.setAnchor(row * columns);
      if (albums) albums.onShift = (index) => {
        if (scroller) scroller.scrollTop = Math.ceil(index / columns) * rowHeight + anchorOffset;
      };
    });
  });

  $effect(() => {
    void visible;
    void width;
    const frame = requestAnimationFrame(() => (cardHeight = list?.querySelector('li')?.offsetHeight ?? 0));
    return () => cancelAnimationFrame(frame);
  });
</script>

<section class="album-grid">
  <div class="zoom" role="group" aria-label="Columns">
    <button class="button" aria-label="Fewer columns" disabled={columns <= 1} onclick={() => choose(columns - 1)}>−</button>
    <span>{columns}</span>
    <button class="button" aria-label="More columns" disabled={columns >= maxColumns} onclick={() => choose(columns + 1)}>+</button>
    <button class="button" disabled={chosen === undefined} onclick={() => choose(undefined)}>
      {layoutColumns ? 'Reset' : 'Auto'}
    </button>
  </div>
  <div
    class="scroll"
    bind:this={scroller}
    bind:clientHeight={viewHeight}
    onscroll={(e) => (scrollTop = (e.currentTarget as HTMLElement).scrollTop)}
  >
    <ul
      bind:this={list}
      bind:clientWidth={width}
      style:--card="{cardSize}px"
      style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
      style:padding-top="{GAP + firstRow * rowHeight}px"
      style:padding-bottom="{GAP + (rowCount - lastRow) * rowHeight}px"
    >
      {#each visible as i (card(i)?.key ?? `#${i}`)}
        {@const album = card(i)}
        <li>
          {#if album}
            <button
              class="card"
              class:playing={album.key === playingKey}
              class:focused={album.key === view.browse.focusAlbum}
              title="{album.title} — {album.artist} · {album.tracks} tracks · {formatTime(album.duration_ms)}"
              onclick={() => (view.browse.focusAlbum = album.key)}
              ondblclick={() => play(album)}
            >
              <Artwork track={album.track} art={album.art} title={album.title} />
              <span class="title">{album.title || 'Unknown album'}</span>
              <span class="artist">{album.artist || 'Unknown artist'}</span>
            </button>
          {:else}
            <div class="card loading" aria-hidden="true"><Artwork track={undefined} art={null} /></div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if count === 0 && albums?.loaded !== false}
      <div class="empty">
        <p>{library.status?.roots.length ? 'No albums found.' : 'No music folders yet.'}</p>
        {#if !library.status?.roots.length}
          <button class="button primary" onclick={() => addFolder(engine)}>Add folder…</button>
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .album-grid {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  .zoom {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: var(--space-1) 16px;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  ul {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(var(--card), 100%), 1fr));
    gap: 16px;
    margin: 0;
    padding: 0 16px;
    list-style: none;
  }

  .card {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 0 0 var(--space-2);
    /* A border, not a box-shadow: blurred shadows are slow to repaint when
       WebKitGTK draws on the CPU, and the grid repaints on every scroll. */
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    text-align: left;
    cursor: pointer;
    outline: 2px solid transparent;
    outline-offset: 1px;
  }

  .card :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  .card:hover {
    outline-color: var(--border);
  }

  .card.focused {
    outline-color: var(--text-muted);
  }

  .card.playing {
    outline-color: var(--accent);
  }

  .title,
  .artist {
    padding: 0 var(--space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    margin-top: var(--space-2);
    font-weight: 500;
  }

  .artist {
    font-size: 0.88em;
    color: var(--text-muted);
  }

  .loading {
    min-height: calc(var(--card) + 40px);
    cursor: default;
  }

  .empty {
    padding: var(--space-6) var(--space-4);
    text-align: center;
    color: var(--text-muted);
  }
</style>
