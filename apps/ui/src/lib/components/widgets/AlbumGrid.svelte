<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { groupConsecutive } from '../../albums';
  import { getApp, getViewState } from '../../context';
  import { demoLibrary, isDemo } from '../../demo';
  import { addFolder } from '../../folders';
  import { formatTime } from '../../format';
  import { hold, type LibraryAlbums, type Shown } from '../../library.svelte';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { navTarget, scrollInto } from '../../keynav';
  import { menu } from '../../contextMenu.svelte';
  import { albumMenu } from '../../menus';
  import { sortLabel, viewSort } from '../../sort.svelte';
  import Artwork from '../player/Artwork.svelte';
  import Scrollbar from '../Scrollbar.svelte';

  let { cardSize, columns: layoutColumns }: { cardSize: number; columns?: number } = $props();

  const { engine, library } = getApp();
  const view = getViewState();

  interface Card {
    key: string;
    title: string;
    artist: string;
    tracks: number;
    duration_ms: number;
    year?: number | null;
    art?: string | null;
    /** For the placeholder artwork when there is no real artwork. */
    track?: TrackRef;
  }

  // Development's demo library, grouped into albums.
  const demoAlbums: Card[] = isDemo
    ? groupConsecutive(demoLibrary).map((g) => ({ ...g, tracks: g.count, track: demoLibrary[g.start] }))
    : [];
  const sort = $derived(viewSort(view));
  const shownList: Shown<LibraryAlbums> = {};
  const nextAlbums = $derived(isDemo ? null : library.albums(view.browse.search || null, sort));
  const albums = $derived(nextAlbums && hold(nextAlbums, shownList));
  $effect(() => albums?.sync());
  const count = $derived(albums ? albums.total : demoAlbums.length);
  const card = (i: number): Card | undefined => (albums ? albums.get(i) : demoAlbums[i]);

  function play(album: Card) {
    if (album.track) {
      const start = demoLibrary.indexOf(album.track);
      engine.run({
        type: 'set_context',
        tracks: demoLibrary.slice(start, start + album.tracks),
        start: 0,
        kind: 'album',
        label: album.title,
      });
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

  // The user's choice (Ctrl+scroll) beats the layout's `columns`, which
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

  // A new order or search starts at the top.
  let shownAlbums: object | null | undefined;
  $effect(() => {
    const next = albums;
    untrack(() => {
      if (shownAlbums && next !== shownAlbums && scroller) scroller.scrollTop = 0;
      shownAlbums = next;
    });
  });

  // Roving focus: one card is in the tab order. If it is virtualised away, the
  // first rendered card stands in so Tab still lands in the grid.
  let cursor = $state(0);
  const tabStop = $derived(visible.includes(cursor) ? cursor : (visible[0] ?? -1));

  async function onKey(e: KeyboardEvent, i: number, album: Card) {
    if (e.key === 'Enter') {
      e.preventDefault(); // the button's own Enter would only select
      play(album);
      return;
    }
    const page = Math.max(1, Math.floor((scroller?.clientHeight ?? viewHeight) / rowHeight));
    const to = navTarget(e, i, count, columns, page);
    if (to === null || !scroller) return;
    e.preventDefault();
    cursor = to;
    viewHeight = scroller.clientHeight; // the bound value can lag a layout change
    const top = Math.floor(to / columns) * rowHeight + GAP;
    scroller.scrollTop = scrollInto(scroller.scrollTop, scroller.clientHeight, top - GAP, top + rowHeight - GAP);
    scrollTop = scroller.scrollTop;
    await tick();
    const card = () => list?.querySelector<HTMLElement>(`[data-index="${to}"]`);
    if (!card()) await new Promise(requestAnimationFrame);
    const el = card();
    if (!el) return;
    el.focus({ preventScroll: true });
    // The row height is an estimate until the artwork has loaded; correct by the real position.
    const c = el.getBoundingClientRect();
    const v = scroller.getBoundingClientRect();
    if (c.top < v.top) scroller.scrollTop -= v.top - c.top;
    else if (c.bottom > v.bottom) scroller.scrollTop += c.bottom - v.bottom;
  }

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

  // The scrollbar snaps to rows and names the top row's sort value; albums
  // only know their artist, title and year.
  const snaps = $derived(Array.from({ length: rowCount }, (_, row) => row * rowHeight));
  function topLabel() {
    const key = sort[0]?.key;
    const album = card(Math.round(scrollTop / rowHeight) * columns);
    if (!album || !['album_artist', 'artist', 'album', 'year'].includes(key)) return undefined;
    return sortLabel(key, { album_artist: album.artist, artist: album.artist, album: album.title, year: album.year });
  }

  $effect(() => {
    void visible;
    void width;
    const frame = requestAnimationFrame(() => (cardHeight = list?.querySelector('li')?.offsetHeight ?? 0));
    return () => cancelAnimationFrame(frame);
  });
</script>

<section class="album-grid">
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
              data-index={i}
              tabindex={i === tabStop ? 0 : -1}
              onfocus={() => (cursor = i)}
              onkeydown={(e) => onKey(e, i, album)}
              use:menu={() => albumMenu(album, () => play(album))}
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
  <Scrollbar target={scroller} {snaps} label={topLabel} />
</section>

<style>
  .album-grid {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
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

  .card:focus-visible {
    outline-color: var(--text);
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
