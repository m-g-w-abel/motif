<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { contextMenu, type MenuItem } from '../../contextMenu.svelte';
  import { libraryMenu } from '../../menus';
  import { DEFAULT_RULES } from '../../sort.svelte';
  import { viewOptions, type OptionsKey } from '../../viewOptions';
  import { CATEGORIES, viewTitle } from '../../views.svelte';
  import { canDecorate } from '../../windowChrome';
  import Icon from '../Icon.svelte';
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import PlaylistPage from '../playlists/PlaylistPage.svelte';
  import RecentlyPlayed from '../widgets/RecentlyPlayed.svelte';
  import NavButton from './NavButton.svelte';
  import SortMenu from './SortMenu.svelte';
  import ViewOptionsSheet from './ViewOptionsSheet.svelte';
  import WindowButtons from './WindowButtons.svelte';

  // Every view but All Songs, which the layouts arrange (ContentArea). The
  // parent re-creates this component for each view.
  const { engine, layout, settings, views, playlists } = getApp();
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

  // A category's header: its round icon and title, then the toolbar.
  const category = CATEGORIES.find((c) => c.kind === view.kind);
  // Recently Played and Added show their songs or their albums.
  let show = $state<'songs' | 'albums'>('songs');
  const recents = view.kind === 'played' || view.kind === 'added';
  const optionsKey: OptionsKey | null =
    view.kind === 'albums' || view.kind === 'playlists' || view.kind === 'played' || view.kind === 'added' ? view.kind : null;
  const collection = $derived(view.kind === 'albums' || view.kind === 'playlists' || (recents && show === 'albums'));
  const options = $derived(optionsKey && viewOptions(settings.current.viewOptions, optionsKey));
  // Albums sort like songs, each kind by its own rules (viewOptions.ts).
  const sortDefaults = optionsKey && viewOptions({}, optionsKey).sort;
  const sort = $derived(options?.sort ?? undefined);
  // The library breaks ties by path: its own tiers keep an album's tracks in disc and track order.
  const order = $derived(sort && [...sort, ...DEFAULT_RULES.filter((d) => !sort.some((r) => r.key === d.key))]);
  let grid = $state<AlbumGrid>();
  let shownColumns = $state(1);

  // Search in the view: the grid's own search, which outlives the view (cleared on closing the box).
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

  const playAlbum = (key: string) =>
    engine.query({ type: 'play_album', album: key, sort: [] }).catch((e: Error) => (engine.error = e.message));
</script>

<section class="view" aria-label={title}>
  <header class="toolbar" {...drag}>
    <NavButton />
    {#if category}<span class="grow" {...drag}></span>{:else}<h1 {...drag}>{title}</h1>{/if}
    {#if view.kind === 'playlists' && engine.can('library')}
      <button class="icon-button" aria-label="New playlist" title="New playlist" onclick={() => playlists.create()}>
        <Icon name="plus" size={16} />
      </button>
    {:else if playlist}
      <button
        class="icon-button"
        aria-label="{playlist.name} menu"
        title="More"
        aria-haspopup="menu"
        onclick={(e) => openBelow(e, playlists.menu(playlist, () => views.back()))}
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
      <div class="actions">
        {#if collection}
          {#if engine.can('playback')}
            <button class="tool" aria-label="Shuffle" title="Shuffle" onclick={() => grid?.playEverything(true)}>
              <Icon name="shuffle" size={18} />
            </button>
            <button class="tool" aria-label="Play" title="Play" onclick={() => grid?.playEverything()}>
              <Icon name="play" size={18} />
            </button>
          {/if}
          {#if searchable}
            <button class="tool" class:active={searching} aria-label="Search in {category.label}" title="Search" aria-pressed={searching} onclick={toggleSearch}>
              <Icon name="search" size={18} />
            </button>
          {/if}
        {/if}
        {#if recents}
          <div class="segmented" role="radiogroup" aria-label="Show">
            {#each [['songs', 'Songs'], ['albums', 'Albums']] as const as [value, label] (value)}
              <button role="radio" aria-checked={show === value} class:active={show === value} onclick={() => (show = value)}>{label}</button>
            {/each}
          </div>
        {/if}
        <span class="grow"></span>
        {#if collection && optionsKey && sort && sortDefaults}
          <SortMenu rules={sort} defaults={sortDefaults} onchange={(rules) => settings.setView(optionsKey, { sort: rules })} />
        {/if}
        {#if collection && optionsKey}
          <button
            class="tool"
            aria-label="{category.label} options"
            title="More"
            aria-haspopup="dialog"
            aria-expanded={views.options}
            onclick={() => (views.options = !views.options)}
          >
            <Icon name="more" size={18} />
          </button>
          {#if views.options}
            <ViewOptionsSheet
              title={category.label}
              key={optionsKey}
              {shownColumns}
              artistsLabel={view.kind === 'playlists' ? 'Show song counts' : 'Show artists'}
              onclose={() => (views.options = false)}
            />
          {/if}
        {/if}
      </div>
      {#if searching && collection && searchable}
        <input
          class="search"
          type="search"
          placeholder="Search {category.label.toLowerCase()}"
          aria-label="Search {category.label}"
          use:focus
          value={query}
          oninput={(e) => search(e.currentTarget.value)}
          onkeydown={(e) => e.key === 'Escape' && (e.stopPropagation(), toggleSearch())}
        />
      {/if}
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
        oncolumns={(columns) => settings.setView(optionsKey, { columns })}
        onopen={(key, title) =>
          views.open(view.kind === 'playlists' ? { kind: 'playlist', id: key, name: title } : { kind: 'album', key, title })}
      />
    {:else if view.kind === 'played'}
      <RecentlyPlayed />
    {:else if view.kind === 'album'}
      <div class="placeholder">
        <button class="button primary" onclick={() => playAlbum(view.key)}><Icon name="play" size={14} /> Play album</button>
        <p>The album's songs show here soon.</p>
      </div>
    {:else if view.kind === 'playlist'}
      <PlaylistPage id={view.id} name={title} />
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
