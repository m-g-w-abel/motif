<script lang="ts">
  import { untrack } from 'svelte';
  import { albumKey } from '../../albums';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { contextMenu, type MenuItem } from '../../contextMenu.svelte';
  import { demoLibrary, isDemo } from '../../demo';
  import type { LibraryTracks, Shown } from '../../library.svelte';
  import { libraryMenu } from '../../menus';
  import type { LibraryTrack } from '../../protocol/LibraryTrack';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { albumLine, matches } from '../../songs';
  import { DEFAULT_RULES, sortRules, viewSort } from '../../sort.svelte';
  import { fixedData, playedData, trackListData } from '../../sources.svelte';
  import { viewOptions, type OptionsKey } from '../../viewOptions';
  import { CATEGORIES, viewTitle } from '../../views.svelte';
  import { canDecorate } from '../../windowChrome';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import SongList from '../widgets/SongList.svelte';
  import PlaylistPage from '../playlists/PlaylistPage.svelte';
  import NavButton from './NavButton.svelte';
  import SortMenu from './SortMenu.svelte';
  import ViewOptionsSheet from './ViewOptionsSheet.svelte';
  import WindowButtons from './WindowButtons.svelte';

  // Every view but desktop's All Songs, which the layouts arrange (ContentArea).
  // The parent re-creates this component for each view.
  const { engine, library, layout, settings, views, playlists } = getApp();
  const view = untrack(() => views.top);

  // The Albums view's grid keeps its own search and selection, apart from All Songs'.
  const area: ViewState = $state({ source: 'library', layoutId: `view:${view.kind}`, browse: layout.browseState(`view:${view.kind}`) });
  setViewState(area);

  const own = $derived(settings.current.ownDecorations && canDecorate);
  const drag = $derived(own ? { 'data-tauri-drag-region': '' } : {});

  /** Opens the menu `items` below the button pressed. */
  function openBelow(e: MouseEvent, items: MenuItem[]) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    contextMenu.show(items, box.right - 200, box.bottom + 4, button);
  }

  // A playlist's page shows its current name: renames come from any device.
  const playlist = $derived(view.kind === 'playlist' ? playlists.list.find((p) => p.id === view.id) : undefined);
  const title = $derived(playlist?.name ?? viewTitle(view));
  // The playlist's own page for dragging songs into order and removing them.
  let editing = $state(false);
  const playlistMenu = $derived(
    playlist && [
      ...playlists.menu(playlist, () => views.back()),
      ...(engine.can('library') && !editing ? [{ label: 'Edit songs…', run: () => (editing = true) }] : []),
    ],
  );

  // A category's header: its round icon and title, then the toolbar.
  const category = CATEGORIES.find((c) => c.kind === view.kind);
  // Recently Played and Added show their songs or their albums.
  let show = $state<'songs' | 'albums'>('songs');
  const recents = view.kind === 'played' || view.kind === 'added';
  const collection = $derived(view.kind === 'albums' || view.kind === 'playlists' || (recents && show === 'albums'));
  const page = view.kind === 'album' || view.kind === 'playlist';
  const songView = $derived(!collection && (view.kind === 'songs' || page || recents));
  const optionsKey = $derived.by((): OptionsKey | null => {
    if (view.kind === 'albums' || view.kind === 'playlists') return view.kind;
    if (recents) return show === 'albums' ? view.kind : `${view.kind}:songs`;
    if (view.kind === 'playlist') return `playlist:${view.id}`;
    return view.kind === 'songs' || view.kind === 'album' ? view.kind : null;
  });
  const options = $derived(optionsKey && viewOptions(settings.current.viewOptions, optionsKey));
  // Albums sort like songs, each kind by its own rules (viewOptions.ts).
  const sortDefaults = $derived(collection && optionsKey && viewOptions({}, optionsKey).sort);
  const sort = $derived(collection ? (options?.sort ?? undefined) : undefined);
  // The library breaks ties by path: its own tiers keep an album's tracks in disc and track order.
  const order = $derived(sort && [...sort, ...DEFAULT_RULES.filter((d) => !sort.some((r) => r.key === d.key))]);
  let grid = $state<AlbumGrid>();
  let songList = $state<SongList>();
  let shownColumns = $state(1);
  let selecting = $state(false);

  // Search in the view: the view's own search, which outlives the view (cleared on closing the box).
  const searchable = view.kind !== 'played';
  let searching = $state(!!area.browse.search);
  let query = $state(area.browse.search);
  let typing: ReturnType<typeof setTimeout> | undefined;
  function search(text: string) {
    query = text;
    clearTimeout(typing);
    typing = setTimeout(() => (area.browse.search = text.trim()), 250);
  }
  // The box opens on a press of Search, to type in.
  const focus = (node: HTMLElement) => node.focus();
  function toggleSearch() {
    searching = !searching;
    if (!searching) search('');
  }

  // An album's or playlist's songs are all fetched; the page's search filters them here.
  let rows = $state.raw<{ track: TrackRef; details?: LibraryTrack }[]>([]);
  $effect(() => {
    if (view.kind === 'album') {
      if (isDemo) {
        rows = demoLibrary.filter((t) => albumKey(t) === view.key).map((track) => ({ track }));
        return;
      }
      void engine.library?.revision;
      engine
        .query({ type: 'album_tracks', albums: [view.key], sort: [] })
        .then((r) => r.type === 'tracks' && (rows = r.tracks.map((details) => ({ track: details.track, details }))))
        .catch(() => {});
    } else if (view.kind === 'playlist' && playlists.revision) {
      playlists
        .entries(view.id)
        .then((entries) => (rows = entries.map((e) => ({ track: e.track.track, details: e.track }))))
        .catch(() => {});
    }
  });
  const shown = $derived(rows.filter((r) => matches(r.track, area.browse.search)));
  // Unfiltered pages play the album or playlist itself, which the engine keeps up to date; a search's result plays as listed.
  function playRows(start: number) {
    if (shown.length === rows.length && view.kind === 'playlist') return playlists.play(view.id, start);
    if (shown.length === rows.length && view.kind === 'album' && !isDemo) {
      engine.query({ type: 'play_album', album: view.key, sort: [], start }).catch((e: Error) => (engine.error = e.message));
      return;
    }
    const kind = view.kind === 'album' ? 'album' : 'list';
    engine.run({ type: 'set_context', tracks: shown.map((r) => r.track), start, kind, label: title });
  }

  // Recently Added's songs, newest first, as its albums.
  const ADDED = viewOptions({}, 'added').sort ?? [];
  const shownList: Shown<LibraryTracks> = {};
  const songs = $derived.by(() => {
    if (page) return fixedData(engine, title, shown, playRows, area.browse.search ? 'No songs match.' : 'No songs here yet.');
    if (view.kind === 'played') return playedData(engine, library.recentPlays);
    const rules = view.kind === 'added' ? ADDED : viewSort(area);
    return trackListData(engine, library, 'library', area.browse.search || null, rules, shownList);
  });
  const openAlbum = (key: string, title: string) => views.open({ kind: 'album', key, title });
  const first = $derived(rows[0]);
</script>

{#snippet toolbar()}
  <div class="actions">
    {#if collection || songView}
      {#if engine.can('playback')}
        <button class="tool" aria-label="Shuffle" title="Shuffle" onclick={() => (collection ? grid : songList)?.playEverything(true)}>
          <Icon name="shuffle" size={18} />
        </button>
        <button class="tool" aria-label="Play" title="Play" onclick={() => (collection ? grid : songList)?.playEverything()}>
          <Icon name="play" size={18} />
        </button>
      {/if}
      {#if searchable}
        <button class="tool" class:active={searching} aria-label="Search in {title}" title="Search" aria-pressed={searching} onclick={toggleSearch}>
          <Icon name="search" size={18} />
        </button>
      {/if}
      {#if songView}
        <button class="tool text" class:active={selecting} aria-pressed={selecting} onclick={() => (selecting = !selecting)}>Select</button>
      {/if}
    {/if}
    {#if recents}
      <div class="segmented" role="radiogroup" aria-label="Show">
        {#each [['songs', 'Songs'], ['albums', 'Albums']] as const as [value, label] (value)}
          <button
            role="radio"
            aria-checked={show === value}
            class:active={show === value}
            onclick={() => {
              show = value;
              selecting = false;
            }}>{label}</button
          >
        {/each}
      </div>
    {/if}
    <span class="grow"></span>
    {#if collection && optionsKey && sort && sortDefaults}
      <SortMenu rules={sort} defaults={sortDefaults} onchange={(rules) => settings.setView(optionsKey, { sort: rules })} />
    {:else if view.kind === 'songs'}
      <SortMenu rules={sortRules.rules[area.source]} defaults={DEFAULT_RULES} onchange={(rules) => sortRules.set(area.source, rules)} />
    {/if}
    {#if optionsKey}
      <button
        class="tool"
        aria-label="{title} options"
        title="More"
        aria-haspopup="dialog"
        aria-expanded={views.options}
        onclick={() => (views.options = !views.options)}
      >
        <Icon name="more" size={18} />
      </button>
      {#if views.options}
        <ViewOptionsSheet
          {title}
          key={optionsKey}
          {shownColumns}
          artistsLabel={view.kind === 'playlists' ? 'Show song counts' : 'Show artists'}
          onclose={() => (views.options = false)}
        />
      {/if}
    {/if}
  </div>
  {#if searching && searchable}
    <input
      class="search"
      type="search"
      placeholder="Search {title.toLowerCase()}"
      aria-label="Search {title}"
      use:focus
      value={query}
      oninput={(e) => search(e.currentTarget.value)}
      onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), toggleSearch())}
    />
  {/if}
{/snippet}

{#snippet hero()}
  <div class="hero">
    <div class="cover">
      <Artwork track={first?.track} art={playlist ? playlist.art : first?.details ? first.details.art : undefined} {title} />
    </div>
    <h2>{title}</h2>
    <p class="line">
      {#if view.kind === 'album'}{first ? albumLine(first.track, first.details) : ''}{:else}{rows.length}
        {rows.length === 1 ? 'song' : 'songs'}{/if}
    </p>
    {@render toolbar()}
  </div>
{/snippet}

<section class="view" aria-label={title}>
  <header class="toolbar" {...drag}>
    <NavButton />
    {#if category}<span class="grow" {...drag}></span>{:else}<h1 {...drag}>{title}</h1>{/if}
    {#if view.kind === 'playlists' && engine.can('library')}
      <button class="icon-button" aria-label="New playlist" title="New playlist" onclick={() => playlists.create()}>
        <Icon name="plus" size={16} />
      </button>
    {:else if playlist && editing}
      <button class="button primary" onclick={() => (editing = false)}>Done</button>
    {:else if playlist && playlistMenu}
      <button
        class="icon-button"
        aria-label="{playlist.name} menu"
        title="More"
        aria-haspopup="menu"
        onclick={(e) => openBelow(e, playlistMenu)}
      >
        <Icon name="more" size={16} />
      </button>
    {/if}
    {#if view.kind === 'home'}
      <button class="icon-button" aria-label="More" title="More" aria-haspopup="menu" onclick={(e) => openBelow(e, libraryMenu(engine))}>
        <Icon name="more" size={16} />
      </button>
    {/if}
    <WindowButtons />
  </header>

  {#if category}
    <div class="head">
      <div class="title-row">
        <span class="dot" style:--c={category.colour}><Icon name={category.icon} size={22} /></span>
        <h1>{category.label}</h1>
      </div>
      {@render toolbar()}
    </div>
  {/if}

  <div class="body">
    {#if view.kind === 'home'}
      <ul class="categories">
        {#each CATEGORIES as category (category.kind)}
          <li>
            <button onclick={() => views.open({ kind: category.kind })}>
              <span class="dot" style:--c={category.colour}><Icon name={category.icon} size={18} /></span>
              {category.label}
            </button>
          </li>
        {/each}
      </ul>
    {:else if collection && options && optionsKey}
      <AlbumGrid
        bind:this={grid}
        bind:shownColumns
        cardSize={150}
        items={view.kind === 'playlists' ? 'playlists' : view.kind === 'played' ? 'played' : 'albums'}
        sort={order}
        mode={options.mode}
        columns={options.columns ?? undefined}
        titles={options.titles}
        artists={options.artists}
        oncolumns={(columns) => optionsKey && settings.setView(optionsKey, { columns })}
        onopen={(key, title) =>
          views.open(view.kind === 'playlists' ? { kind: 'playlist', id: key, name: title } : { kind: 'album', key, title })}
      />
    {:else if view.kind === 'playlist' && editing}
      <PlaylistPage id={view.id} name={title} />
    {:else if songView && options}
      <SongList
        bind:this={songList}
        bind:shownColumns
        bind:selecting
        data={songs}
        mode={options.mode}
        columns={options.columns ?? undefined}
        titles={options.titles}
        artists={options.artists}
        inAlbum={view.kind === 'album'}
        header={page ? hero : undefined}
        openAlbum={view.kind === 'album' ? undefined : openAlbum}
      />
    {:else}
      <p class="placeholder">{viewTitle(view)} shows here soon.</p>
    {/if}
  </div>
</section>

<style>
  .view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 36px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex: none;
  }

  h1 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 1em;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .categories {
    margin: 0;
    padding: var(--space-2) 0;
    list-style: none;
  }

  .categories button {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--text);
    font-size: 1.15em;
    text-align: left;
    cursor: pointer;
  }

  .categories button:hover {
    background: var(--hover);
  }

  .dot {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    background: var(--c);
    color: #fff;
    flex: none;
  }

  .grow {
    flex: 1;
    align-self: stretch;
  }

  .head {
    padding: var(--space-3) var(--space-3) var(--space-2);
    flex: none;
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .title-row .dot {
    width: 48px;
    height: 48px;
  }

  .head h1 {
    flex: 1;
    font-size: 1.8em;
  }

  .actions {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }

  .tool {
    display: grid;
    place-items: center;
    width: 44px;
    height: 36px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: none;
    color: var(--text);
    cursor: pointer;
  }

  /* The sort button matches the toolbar's. */
  .actions :global(.menu > .icon-button) {
    width: 44px;
    height: 36px;
    border: 1px solid var(--border);
    color: var(--text);
  }

  .tool.text {
    width: auto;
    padding: 0 var(--space-3);
    font-weight: 600;
  }

  .tool:hover,
  .tool.active {
    background: var(--hover);
  }

  .segmented {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .segmented button {
    height: 36px;
    padding: 0 var(--space-3);
    border: 0;
    border-right: 1px solid var(--border);
    background: none;
    cursor: pointer;
  }

  .segmented button:last-child {
    border-right: 0;
  }

  .segmented button.active {
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
  }

  .search {
    width: 100%;
    height: 36px;
    margin-top: var(--space-2);
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
  }

  /* An album's or playlist's page: its cover, title and line, then the toolbar; it scrolls with the songs. */
  .hero {
    padding: var(--space-3) var(--space-3) var(--space-2);
    text-align: center;
  }

  .cover {
    width: min(100%, 320px);
    margin: 0 auto var(--space-3);
  }

  .cover :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm);
  }

  .hero h2 {
    margin: 0;
    font-size: 1.4em;
  }

  .line {
    margin: var(--space-1) 0 0;
    color: var(--text-muted);
  }

  .hero .actions {
    text-align: left;
  }

  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    padding: var(--space-4) var(--space-3);
    color: var(--text-muted);
  }
</style>
