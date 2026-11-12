<script lang="ts">
  import { tick, untrack } from 'svelte';
  import { groupConsecutive } from '../../albums';
  import { getApp, getViewState } from '../../context';
  import { demoLibrary, isDemo } from '../../demo';
  import { addFolder } from '../../folders';
  import { formatTime } from '../../format';
  import { gesture, mapped, swipeRow } from '../../gestures';
  import { hold, type LibraryAlbums, type Shown } from '../../library.svelte';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { navTarget, scrollInto, vimTarget } from '../../keynav';
  import { menu } from '../../contextMenu.svelte';
  import type { MenuItem } from '../../contextMenu.svelte';
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import type { SortRule } from '../../protocol/SortRule';
  import type { ViewMode } from '../../viewOptions';
  import { inSelection, ordered, span, toggle, type ItemAt } from '../../selection';
  import { sortLabel, viewSort } from '../../sort.svelte';
  import Artwork from '../player/Artwork.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { scrollMemory } from '../../scrollMemory';
  
  let {
    cardSize,
    columns: layoutColumns,
    at,
    onopen,
    items = 'albums',
    sort: sortOverride,
    mode = 'grid',
    titles = true,
    artists = true,
    oncolumns,
    shownColumns = $bindable(),
  }: {
    cardSize: number;
    columns?: number;
    /** Where the grid sits in its layout; its scroll position is remembered per layout. */
    at?: string;
    /** Opens an album's page on a plain click or Enter, instead of selecting it (the Albums view). */
    onopen?: (key: string, title: string, art?: string | null) => void;
    /** What the cards are: the library's albums, recently played albums, or the saved playlists. */
    items?: 'albums' | 'played' | 'playlists';
    /** The order instead of the view's (library albums only). */
    sort?: SortRule[];
    /** Cover grid, or one row per item: a name list, or a list with covers. */
    mode?: ViewMode;
    /** Grid only: titles and artists under the covers. */
    titles?: boolean;
    artists?: boolean;
    /** Takes the user's column choice (Ctrl+scroll) instead of remembering it per layout. */
    oncolumns?: (n: number | null) => void;
    /** The columns shown, for a view's options. */
    shownColumns?: number;
  } = $props();
  /** What its cards open, for their cover and title to fly to the page (`screens.ts`). */
  const kind = $derived(items === 'playlists' ? 'playlist' : 'album');

  const { engine, library, settings, playlists } = getApp();
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
  const sort = $derived(sortOverride ?? viewSort(view));
  const search = $derived(view.browse.search.trim().toLowerCase());
  // Playlists are few and all here; the library's albums come a page at a time.
  const fixed = $derived.by((): Card[] | null => {
    if (items === 'playlists') {
      return playlists.list
        .filter((p) => p.name.toLowerCase().includes(search))
        .map((p) => ({ key: p.id, title: p.name, artist: `${p.tracks} songs`, tracks: p.tracks, duration_ms: p.duration_ms, art: p.art }));
    }
    return isDemo ? (items === 'played' ? [] : demoAlbums) : null;
  });
  $effect(() => {
    if (items === 'playlists') playlists.sync();
  });
  const shownList: Shown<LibraryAlbums> = {};
  const nextAlbums = $derived(
    fixed ? null : items === 'played' ? library.recentAlbums : library.albums(view.browse.search || null, sort),
  );
  const albums = $derived(nextAlbums && hold(nextAlbums, shownList));
  $effect(() => albums?.sync());
  const count = $derived(albums ? albums.total : fixed!.length);
  const card = (i: number): Card | undefined => (albums ? albums.get(i) : fixed![i]);

  function play(album: Card) {
    if (items === 'playlists') {
      playlists.play(album.key);
    } else if (album.track) {
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
    if (items === 'playlists') return (await Promise.all(keys.map((id) => playlists.entries(id)))).flat().map((e) => e.track.track);
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

  /** Plays every item, in order or shuffled: the engine's shuffle, its first track a random one. */
  export async function playEverything(shuffle = false) {
    try {
      await engine.send({ type: 'set_shuffle', on: shuffle });
      const from = (total: number) => (shuffle ? Math.floor(Math.random() * total) : 0);
      if (items === 'albums' && !isDemo) {
        const search = view.browse.search || null;
        const found = await engine.query({ type: 'tracks', search, sort, offset: 0, limit: 0 });
        if (found.type === 'tracks' && found.total) await engine.query({ type: 'play', search, sort, start: from(found.total) });
        return;
      }
      // shortcut: all playlists play as a snapshot of their tracks (one playlist at a time is followed live); recent albums are a snapshot by design.
      await albums?.load(0, count - 1);
      const keys = Array.from({ length: count }, (_, i) => card(i)?.key).filter((k) => k !== undefined);
      const tracks = await albumTracks(keys);
      if (tracks.length) engine.run({ type: 'set_context', tracks, start: from(tracks.length), kind: 'list', label: null });
    } catch (e) {
      engine.error = (e as Error).message;
    }
  }

  // As in the track list: a press inside a selection waits for the click,
  // so a right-click acts on all of it.
  function press(e: PointerEvent, i: number, album: Card) {
    gesture(
      e,
      mapped(settings.current.gestures, 'row'),
      { queue: () => queueAlbum(i, album), 'play-next': () => queueAlbum(i, album, true) },
      swipeRow(e.currentTarget as HTMLElement, 'queue'),
    );
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
  /** Middle-click or long-press: queues the album, or the picked albums it is part of. */
  function queueAlbum(i: number, album: Card, next = false) {
    if (!inSelection(picked, album.key)) select(i);
    const keys = keysFor(album);
    queueTracks(engine, albumTracks(keys), next, keys.length > 1 ? `${keys.length} albums` : album.title || 'Unknown album');
  }
  function release(e: MouseEvent, i: number) {
    if (e.shiftKey || e.ctrlKey || e.metaKey) return;
    const album = card(i);
    if (onopen && album) onopen(album.key, album.title, album.art);
    else select(i);
  }

  function menuFor(album: Card): MenuItem[] {
    const keys = keysFor(album);
    if (items !== 'playlists') return albumMenu(engine, album, () => playAll(album), () => albumTracks(keys), keys.length);
    const label = keys.length > 1 ? `${keys.length} playlists` : album.title;
    const queue: MenuItem[] = engine.can('queue')
      ? [
          { label: 'Queue next', run: () => queueTracks(engine, albumTracks(keys), true, label) },
          { label: 'Add to queue', run: () => queueTracks(engine, albumTracks(keys), false, label) },
        ]
      : [];
    const playlist = keys.length === 1 && playlists.list.find((p) => p.id === album.key);
    if (!playlist) return [...(engine.can('playback') ? [{ label: 'Play', run: () => playAll(album) }] : []), ...queue];
    const menu = playlists.menu(playlist);
    const at = menu.findIndex((item) => item.label === 'Rename…');
    menu.splice(at < 0 ? menu.length : at, 0, ...queue);
    return menu;
  }

  /** The playing track's album, keyed the way the library keys albums. */
  const playingKey = $derived.by(() => {
    const track = engine.current?.track;
    if (!track) return null;
    const artist = library.details(track)?.album_artist ?? track.artist ?? '';
    return `${artist}\u001f${track.album ?? ''}`;
  });

  // Only the rows of cards in view are in the DOM. Columns follow the CSS
  // grid's auto-fill rule; the card height is measured once rendered. The
  // list modes are a grid of one column without gaps.
  const grid = $derived(mode === 'grid');
  const GAP = $derived(grid ? 16 : 0);
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
    if (oncolumns) return;
    try {
      const n = Number(localStorage.getItem(colsKey));
      chosen = n >= 1 ? n : undefined;
    } catch {
      chosen = undefined;
    }
  });
  const wanted = $derived(chosen ?? layoutColumns);
  const columns = $derived(!grid ? 1 : wanted ? Math.min(wanted, maxColumns) : autoColumns);
  $effect(() => {
    shownColumns = columns;
  });
  function choose(n: number | undefined) {
    if (oncolumns) return oncolumns(n === undefined ? null : Math.max(1, Math.min(n, maxColumns)));
    chosen = n === undefined ? undefined : Math.max(1, Math.min(n, maxColumns));
    try {
      if (chosen === undefined) localStorage.removeItem(colsKey);
      else localStorage.setItem(colsKey, String(chosen));
    } catch {}
  }
  // Wheel up = bigger cards = fewer columns.
  function wheel(e: WheelEvent) {
    if (!e.ctrlKey || !grid) return;
    e.preventDefault();
    choose(columns + (e.deltaY > 0 ? 1 : -1));
  }
  $effect(() => {
    scroller?.addEventListener('wheel', wheel, { passive: false });
    return () => scroller?.removeEventListener('wheel', wheel);
  });
  const rowHeight = $derived((cardHeight || (grid ? cardSize + 48 : 48)) + GAP);
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

  let vimPending = '';
  async function onKey(e: KeyboardEvent, i: number, album: Card) {
    if (e.key === 'Enter') {
      e.preventDefault(); // the button's own Enter would only select
      if (onopen) onopen(album.key, album.title, album.art);
      else playAll(album);
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
    let to: number | null = null;
    let vimMove = false;
    if (settings.current.vimKeys) {
      const vim = vimTarget(e, vimPending, i, count, columns, page);
      vimPending = vim.pending;
      if (vim.handled) {
        e.preventDefault();
        if (vim.to === null) return; // a count or `g` waiting
        to = vim.to;
        vimMove = true;
      }
    }
    to ??= navTarget(e, i, count, columns, page);
    if (to === null || !scroller) return;
    e.preventDefault();
    if (e.shiftKey && !vimMove) extend(to);
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
    void mode;
    void titles;
    void artists;
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
      class:rows={!grid}
      class:covers={mode === 'covers'}
      bind:this={list}
      bind:clientWidth={width}
      style:--card="{cardSize}px"
      style:grid-template-columns="repeat({columns}, minmax(0, 1fr))"
      style:padding-top="{GAP + firstRow * rowHeight}px"
      style:padding-bottom="{GAP + (rowCount - lastRow) * rowHeight}px"
      style:gap="{GAP}px"
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
              use:menu={() => menuFor(album)}
              class:playing={album.key === playingKey}
              class:focused={picked.has(album.key) || album.key === view.browse.focusAlbum}
              aria-pressed={picked.has(album.key)}
              title={items === 'playlists'
                ? `${album.title} · ${album.artist} · ${formatTime(album.duration_ms)}`
                : `${album.title} — ${album.artist} · ${album.tracks} tracks · ${formatTime(album.duration_ms)}`}
              onpointerdown={(e) => press(e, i, album)}
              onclick={(e) => release(e, i)}
              onauxclick={(e) => e.button === 1 && queueAlbum(i, album)}
              ondblclick={() => play(album)}
            >
              {#if mode !== 'list'}
                <span class="cover" data-move={`cover:${kind}:${album.key}`}>
                  <Artwork track={album.track} art={album.art} title={album.title} size={grid ? undefined : 40} />
                </span>
              {/if}
              {#if grid}
                {#if titles}<span class="title" data-move={`title:${kind}:${album.key}`}>{album.title || 'Unknown album'}</span>{/if}
                {#if artists}<span class="artist">{album.artist || 'Unknown artist'}</span>{/if}
              {:else}
                <span class="text">
                  <span class="title" data-move={`title:${kind}:${album.key}`}>{album.title || 'Unknown album'}</span>
                  <span class="artist">{album.artist || 'Unknown artist'}</span>
                </span>
              {/if}
            </button>
          {:else}
            <div class="card loading" aria-hidden="true">
              {#if mode !== 'list'}<Artwork track={undefined} art={null} size={grid ? undefined : 40} />{/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if count === 0 && albums?.loaded !== false}
      <div class="empty">
        <p>
          {items === 'playlists'
            ? search ? 'No playlists found.' : 'No playlists yet.'
            : items === 'played'
              ? 'Nothing played yet.'
              : library.status?.roots.length ? 'No albums found.' : 'No music folders yet.'}
        </p>
        {#if items === 'albums' && !library.status?.roots.length && engine.can('library')}
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

  .cover {
    display: block;
    flex: none;
  }

  .card :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  .rows {
    padding: 0;
  }

  .rows .card {
    flex-direction: row;
    align-items: center;
    gap: var(--space-3);
    height: 48px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: 0;
    background: none;
    outline-offset: -2px;
  }

  .covers .card {
    height: 56px;
  }

  .rows .card :global(.art) {
    width: 40px;
    border-radius: var(--radius-sm);
    flex: none;
  }

  .rows .card:hover {
    background: var(--hover);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .rows .title,
  .rows .artist {
    padding: 0;
  }

  .rows .title {
    margin: 0;
  }

  .rows .loading {
    min-height: 0;
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
    margin-top: var(--space-2);
    font-size: 0.88em;
    color: var(--text-muted);
  }

  .title + .artist {
    margin-top: 0;
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
