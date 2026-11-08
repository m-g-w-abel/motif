<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { contextMenu } from '../../contextMenu.svelte';
  import { libraryMenu } from '../../menus';
  import { CATEGORIES, viewTitle } from '../../views.svelte';
  import { canDecorate } from '../../windowChrome';
  import Icon from '../Icon.svelte';
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import NavButton from './NavButton.svelte';
  import WindowButtons from './WindowButtons.svelte';

  // Every view but All Songs, which the layouts arrange (ContentArea). The
  // parent re-creates this component for each view.
  const { engine, layout, settings, settingsView, views } = getApp();
  const view = untrack(() => views.top);

  // The Albums view's grid keeps its own search and selection, apart from All Songs'.
  const state: ViewState = $state({ source: 'library', layoutId: `view:${view.kind}`, browse: layout.browseState(`view:${view.kind}`) });
  setViewState(state);

  const own = $derived(settings.current.ownDecorations && canDecorate);
  const drag = $derived(own ? { 'data-tauri-drag-region': '' } : {});

  function openMenu(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    contextMenu.show(libraryMenu(engine), box.right - 200, box.bottom + 4, button);
  }

  const playAlbum = (key: string) =>
    engine.query({ type: 'play_album', album: key, sort: [] }).catch((e: Error) => (engine.error = e.message));
</script>

<section class="view" aria-label={viewTitle(view)}>
  <header class="toolbar" {...drag}>
    <NavButton />
    <h1 {...drag}>{viewTitle(view)}</h1>
    {#if view.kind === 'home'}
      <!-- shortcut: Settings is here on phones until the bottom bar's menu (#168) holds it. -->
      <button class="icon-button" aria-label="Settings" title="Settings" onclick={() => settingsView.open()}>
        <Icon name="settings" size={16} />
      </button>
      <button class="icon-button" aria-label="More" title="More" aria-haspopup="menu" onclick={openMenu}>
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
    {:else if view.kind === 'album'}
      <div class="placeholder">
        <button class="button primary" onclick={() => playAlbum(view.key)}><Icon name="play" size={14} /> Play album</button>
        <p>The album's songs show here soon.</p>
      </div>
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
