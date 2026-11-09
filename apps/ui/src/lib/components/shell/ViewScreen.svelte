<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { contextMenu, menu, type MenuItem } from '../../contextMenu.svelte';
  import { libraryMenu } from '../../menus';
  import { CATEGORIES, viewTitle } from '../../views.svelte';
  import { canDecorate } from '../../windowChrome';
  import Icon from '../Icon.svelte';
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import PlaylistPage from '../playlists/PlaylistPage.svelte';
  import RecentlyPlayed from '../widgets/RecentlyPlayed.svelte';
  import NavButton from './NavButton.svelte';
  import WindowButtons from './WindowButtons.svelte';

  // Every view but All Songs, which the layouts arrange (ContentArea). The
  // parent re-creates this component for each view.
  const { engine, layout, settings, views, playlists } = getApp();
  const view = untrack(() => views.top);

  // The Albums view's grid keeps its own search and selection, apart from All Songs'.
  const state: ViewState = $state({ source: 'library', layoutId: `view:${view.kind}`, browse: layout.browseState(`view:${view.kind}`) });
  setViewState(state);

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

  const playAlbum = (key: string) =>
    engine.query({ type: 'play_album', album: key, sort: [] }).catch((e: Error) => (engine.error = e.message));
</script>

<section class="view" aria-label={title}>
  <header class="toolbar" {...drag}>
    <NavButton />
    <h1 {...drag}>{title}</h1>
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
    {:else if view.kind === 'albums'}
      <AlbumGrid cardSize={150} onopen={(key, title) => views.open({ kind: 'album', key, title })} />
    {:else if view.kind === 'played'}
      <RecentlyPlayed onopen={(key, title) => views.open({ kind: 'album', key, title })} />
    {:else if view.kind === 'album'}
      <div class="placeholder">
        <button class="button primary" onclick={() => playAlbum(view.key)}><Icon name="play" size={14} /> Play album</button>
        <p>The album's songs show here soon.</p>
      </div>
    {:else if view.kind === 'playlists'}
      <!-- shortcut: a name list until the collection view (#169) shows playlists like albums. -->
      {#if !playlists.list.length}<p class="placeholder">No playlists yet.</p>{/if}
      <ul class="categories">
        {#each playlists.list as p (p.id)}
          <li>
            <button onclick={() => views.open({ kind: 'playlist', id: p.id, name: p.name })} use:menu={() => playlists.menu(p)}>
              <span class="dot plain"><Icon name="list" size={18} /></span>
              <span class="name">{p.name}</span>
              <span class="count">{p.tracks}</span>
            </button>
          </li>
        {/each}
      </ul>
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

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    font-size: 0.8em;
    color: var(--text-muted);
  }

  .plain {
    background: var(--hover);
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
