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
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import { inSelection, ordered, span, toggle, type ItemAt } from '../../selection';
  import { sortLabel, viewSort } from '../../sort.svelte';
  import Artwork from '../player/Artwork.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { scrollMemory } from '../../scrollMemory';

  let {
    cardSize,
    columns: layoutColumns,
    at,
  }: {
    cardSize: number;
    columns?: number;
    /** Where the grid sits in its layout; its scroll position is remembered per layout. */
    at?: string;
  } = $props();

  const { engine, library } = getApp();
  const view = getViewState();
  const memoryKey = $derived(at === undefined ? undefined : `${view.source}/${view.layoutId}/${at}`);

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

  /** The tracks of these albums, album by album. */
  async function albumTracks(keys: string[]): Promise<TrackRef[]> {
    if (isDemo) {
      return keys.flatMap((key) => {
        const album = demoAlbums.find((a) => a.key === key);
        const start = album?.track ? demoLibrary.indexOf(album.track) : -1;
        return start < 0 ? [] : demoLibrary.slice(start, start + album!.tracks);
      });
    }
    const result = await engine.query({ type: 'album_tracks', albums: keys, sort });
    return result.type === 'tracks' ? result.tracks.map((t) => t.track) : [];
  }

  // Picked cards live in shared browse state, like the track list's rows;
  // `focusAlbum` is the last clicked one.
  const picked = $derived(view.browse.pickedAlbums);
  const itemAt: ItemAt<string> = (i) => {
    const key = card(i)?.key;
    return key === undefined ? undefined : [key, key];
  };
  /** Where a Shift-click or Shift+arrow range starts. */
  let anchor = 0;
  function select(i: number) {
    view.browse.pickedAlbums = span(itemAt, i, i);
    view.browse.focusAlbum = card(i)?.key ?? null;
    anchor = i;
  }
  async function extend(i: number) {
    const from = anchor;
    await albums?.load(Math.min(from, i), Math.max(from, i));
    view.browse.pickedAlbums = span(itemAt, from, i);
    view.browse.focusAlbum = card(i)?.key ?? null;
  }
  async function selectAll() {
    await albums?.load(0, count - 1);
    view.browse.pickedAlbums = span(itemAt, 0, count - 1);
  }
  function clear() {
    view.browse.pickedAlbums = new Map();
    view.browse.focusAlbum = null;
  }
  /** The albums an action on `album` applies to: the selection, in grid order, if the album is part of it. */
  const keysFor = (album: Card) => (inSelection(picked, album.key) ? ordered(picked) : [album.key]);
  function playAll(album: Card) {
    if (!inSelection(picked, album.key)) return play(album);
    albumTracks(keysFor(album))
      .then((tracks) => playTracks(engine, tracks))
      .catch((e: Error) => (engine.error = e.message));
  }

  // As in the track list: a press inside a selection waits for the click,
  // so a right-click acts on all of it.
  function press(e: PointerEvent, i: number, album: Card) {
    if (e.button === 2) {
      if (!picked.has(album.key)) select(i);
    } else if (e.button !== 0) {
      return;
    } else if (e.shiftKey) {
      e.preventDefault();
      extend(i);
    } else if (e.ctrlKey || e.metaKey) {
      view.browse.pickedAlbums = toggle(picked, itemAt, i);
      view.browse.focusAlbum = album.key;
      anchor = i;
    } else if (!inSelection(picked, album.key)) select(i);
  }
  function release(e: MouseEvent, i: number) {
    if (!e.shiftKey && !e.ctrlKey && !e.metaKey) select(i);
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
      playAll(album);
      return;
    }
    if (e.key === 'a' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      selectAll();
      return;
    }
    if (e.key === 'Escape' && picked.size > 1) {
      e.stopPropagation(); // the selection goes first, then the sheet or menu
      clear();
      return;
    }
    const page = Math.max(1, Math.floor((scroller?.clientHeight ?? viewHeight) / rowHeight));
    const to = navTarget(e, i, count, columns, page);
    if (to === null || !scroller) return;
    e.preventDefault();
    if (e.shiftKey) extend(to);
    else anchor = to;
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
  // On arrival (the layout appearing) the focused album's card comes into view,
  // else the grid's last position. Waits while the cards before it load.
  let arrived = false;
  $effect(() => {
    if (arrived || !scroller || !count) return;
    const focus = view.browse.focusAlbum;
    const found = focus ? indexOfAlbum(focus) : undefined;
    if (found === 'loading') return;
    arrived = true;
    untrack(() =>
      void measured().then(() => {
        if (!scroller) return;
        const saved = memoryKey ? scrollMemory.get(memoryKey) : undefined;
        if (found !== undefined) {
          cursor = found;
          scrollToCard(found);
        } else if (saved !== undefined) scroller.scrollTop = saved;
      }),
    );
  });

  /** Index of an album; 'loading' while the cards before it are still arriving. */
  function indexOfAlbum(key: string): number | 'loading' | undefined {
    for (let i = 0; i < count; i++) {
      const album = card(i);
      if (!album) return 'loading';
      if (album.key === key) return i;
    }
    return undefined;
  }

  /** Reads the width and card size off the drawn grid and lays it out again: columns and rows depend on them. */
  async function measured() {
    if (list) width = list.clientWidth;
    if (scroller) viewHeight = scroller.clientHeight;
    cardHeight = list?.querySelector('li')?.offsetHeight ?? cardHeight;
    await tick();
  }

  /** Puts a card's row in the middle of the grid. */
  function scrollToCard(index: number) {
    if (!scroller) return;
    // Cards are measured once drawn; until then the estimate stands.
    const row = (list?.querySelector('li')?.offsetHeight || cardHeight || cardSize + 48) + GAP;
    scroller.scrollTop = Math.max(0, Math.floor(index / columns) * row - (scroller.clientHeight - row) / 2);
  }
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
  <!-- Esc does the same from the keyboard. -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="scroll"
    bind:this={scroller}
    bind:clientHeight={viewHeight}
    onclick={(e) => {
      if (!(e.target as Element).closest('.card')) clear();
    }}
    onscroll={(e) => {
      scrollTop = (e.currentTarget as HTMLElement).scrollTop;
      if (memoryKey && arrived) scrollMemory.set(memoryKey, scrollTop);
    }}
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
              use:menu={() =>
                albumMenu(engine, album, () => playAll(album), () => albumTracks(keysFor(album)), keysFor(album).length)}
              class:playing={album.key === playingKey}
              class:focused={picked.has(album.key) || album.key === view.browse.focusAlbum}
              aria-pressed={picked.has(album.key)}
              title="{album.title} — {album.artist} · {album.tracks} tracks · {formatTime(album.duration_ms)}"
              onpointerdown={(e) => press(e, i, album)}
              onclick={(e) => release(e, i)}
              onauxclick={(e) => {
                if (e.button === 1) queueTracks(engine, albumTracks(keysFor(album)));
              }}
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
  {#if picked.size > 1}<div class="count">{picked.size} selected</div>{/if}
</section>

<style>
  .album-grid {
    position: relative;
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

  .count {
    position: absolute;
    bottom: var(--space-3);
    left: 50%;
    transform: translateX(-50%);
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    font-size: 0.88em;
    pointer-events: none;
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
