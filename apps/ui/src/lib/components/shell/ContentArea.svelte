<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { isDemo } from '../../demo';
  import { formatTime } from '../../format';
  import { installLayoutFiles } from '../../layout/install';
  import { toasts } from '../../toast.svelte';
  import { DEFAULT_RULES, sortRules, viewSort } from '../../sort.svelte';
  import type { LibraryTracks, Shown } from '../../library.svelte';
  import { trackListData } from '../../sources.svelte';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { rootPath } from '../../layout/overlay';
  import LayoutNode from './LayoutNode.svelte';
  import { kdlAccept, isAndroid } from '../../platform';
  import { canDecorate } from '../../windowChrome';
  import NavButton from './NavButton.svelte';
  import SortMenu from './SortMenu.svelte';
  import WindowButtons from './WindowButtons.svelte';

  let { onEditLayout }: { onEditLayout: () => void } = $props();

  const { engine, library, layout, settings, views } = getApp();
  // Phones don't use layouts: All Songs is the default layout's one track list there.
  const shown = $derived(views.phone ? layout.builtins[0].layout.root : layout.shown);

  // The parent re-creates this component when the source or layout changes,
  // so these never change here and each arrangement starts fresh.
  const view: ViewState = $state(
    untrack(() => ({ source: layout.source, layoutId: layout.activeId, browse: layout.browseState(layout.source) })),
  );
  setViewState(view);

  const shownList: Shown<LibraryTracks> = {};
  const data = $derived(
    trackListData(engine, library, view.source, view.browse.search || null, viewSort(view), shownList),
  );
  $effect(() => data.sync?.());

  // The search box filters the library once typing pauses.
  let search = $state(untrack(() => view.browse.search));
  $effect(() => {
    const value = search.trim();
    const timer = setTimeout(() => (view.browse.search = value), 250);
    return () => clearTimeout(timer);
  });

  let menuOpen = $state(false);
  /** The name being typed for Save as…; null while the menu shows its buttons. */
  let saveName = $state<string | null>(null);
  let menu = $state<HTMLElement>();
  let fileInput = $state<HTMLInputElement>();
  let area = $state<HTMLElement>();

  function saveAs(e: SubmitEvent) {
    e.preventDefault();
    const name = saveName?.trim();
    if (!name) return;
    const result = layout.saveAs(name);
    // On failure the form stays open so the name can be changed.
    if (!result.ok) {
      toasts.show(`Couldn't save the layout: ${result.errors[0].message}`, 'error');
      return;
    }
    toasts.show(`Saved layout “${result.entry.layout.name}”`);
    menuOpen = false;
  }

  const own = $derived(settings.current.ownDecorations && canDecorate);
  const drag = $derived(own ? { 'data-tauri-drag-region': '' } : {});
  const focus = (node: HTMLInputElement) => node.select();

  function onWindowPointer(e: PointerEvent) {
    if (menuOpen && menu && !menu.contains(e.target as Node)) menuOpen = false;
  }

</script>

<svelte:window
  onpointerdown={onWindowPointer}
  onkeydown={(e) => {
    if (e.key === 'Escape') menuOpen = false;
  }}
/>

<section class="content" aria-label="All Songs">
  <header class="toolbar" {...drag}>
    <div class="title" {...drag}>
      <NavButton />
      <h1 {...drag}>All Songs</h1>
      <span class="count" {...drag}>
        {data.count} tracks · {formatTime(data.totalMs)}
        {#if library.status?.scanning}{' · '}scanning…{/if}
      </span>
      {#if isDemo}<span class="badge" title="Development demo library (?dev-library)">Demo data</span>{/if}
    </div>

    <div class="search">
      <Icon name="search" size={14} />
      <input type="search" placeholder="Search" aria-label="Search the library"
        bind:value={search}
        onkeydown={(e) => {
          if (e.key === 'Enter' && settings.current.vimKeys)
            document.querySelector<HTMLElement>('main tr.track[tabindex="0"], main button.card[tabindex="0"]')?.focus();
        }}
      />
    </div>

    <SortMenu
      rules={sortRules.rules[view.source]}
      defaults={DEFAULT_RULES}
      onchange={(rules) => {
        // Editing the rules drops a one-off column sort so the edit shows.
        view.browse.oneOff = null;
        sortRules.set(view.source, rules);
      }}
      oneOff={view.browse.oneOff}
      onclearOneOff={() => (view.browse.oneOff = null)}
      tip="Click a column header to sort by it once; this order stays saved."
    />

    {#if layout.arranging && !views.phone}
      <button class="button primary" onclick={() => (layout.arranging = false)}>Done</button>
    {/if}
    {#if !views.phone}
    <div class="layouts" role="radiogroup" aria-label="Layout">
      {#each layout.all as entry (entry.id)}
        <button
          class="icon-button"
          class:active={entry.id === layout.activeId}
          role="radio"
          aria-checked={entry.id === layout.activeId}
          aria-label={entry.layout.name}
          title={entry.layout.description ? `${entry.layout.name} — ${entry.layout.description}` : entry.layout.name}
          onclick={() => layout.select(entry.id)}
        >
          <Icon name={entry.layout.icon} size={16} />
        </button>
      {/each}

      <div class="menu" bind:this={menu}>
        <button
          class="icon-button"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          aria-label="More layout options"
          title="More layout options"
          onclick={() => {
            menuOpen = !menuOpen;
            saveName = null;
          }}
        >
          <Icon name="more" size={16} />
        </button>
        {#if menuOpen}
          <div class="popover" role="menu">
            <div class="current">{layout.layout.name}</div>
            <button
              role="menuitem"
              onclick={() => {
                menuOpen = false;
                layout.arranging = !layout.arranging;
              }}
            >
              {layout.arranging ? 'Stop arranging' : 'Arrange widgets'}
            </button>
            {#if layout.edited}
              <button
                role="menuitem"
                onclick={() => {
                  menuOpen = false;
                  layout.resetEdits();
                }}
              >
                Reset layout
              </button>
            {/if}
            {#if saveName === null}
              <button role="menuitem" onclick={() => (saveName = `${layout.layout.name} (custom)`)}>Save as…</button>
            {:else}
              <form class="save-as" onsubmit={saveAs}>
                <input aria-label="New layout name" bind:value={saveName} use:focus />
                <button type="submit" disabled={!saveName.trim()}>Save</button>
              </form>
            {/if}
            <button role="menuitem" onclick={() => fileInput?.click()}>Add layout file…</button>
            <button
              role="menuitem"
              onclick={() => {
                menuOpen = false;
                onEditLayout();
              }}
            >
              Edit layout file…
            </button>
            {#if !layout.active.builtin}
              <button
                role="menuitem"
                class="danger"
                onclick={() => {
                  layout.remove(layout.activeId);
                  menuOpen = false;
                }}
              >
                Remove “{layout.layout.name}”
              </button>
            {/if}
            {#if !isAndroid}<p class="tip">Or drop a .kdl file onto the window.</p>{/if}
          </div>
        {/if}
      </div>
    </div>
    {/if}

    <WindowButtons />
  </header>

  <div class="area" bind:this={area}>
    <LayoutNode node={shown} path={rootPath(shown)} />
  </div>
  <Scrollbar target={area} />

  <input
    bind:this={fileInput}
    type="file"
    accept={kdlAccept}
    multiple
    hidden
    onchange={async (e) => {
      const input = e.currentTarget;
      await installLayoutFiles(layout, input.files ?? []);
      input.value = '';
      menuOpen = false;
    }}
  />
</section>

<style>
  .content {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    /* Never wider than the window: focusing the search box would otherwise
       scroll the whole view sideways. Only x is clipped so popovers can hang
       below; the z-index lifts them (and any later toolbar menu) over the area,
       including the arranging handles and outlines (LayoutNode, up to 4). */
    overflow-x: clip;
    position: relative;
    z-index: 5;
    height: 36px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex: none;
  }

  .title {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  h1 {
    margin: 0;
    font-size: 1em;
    font-weight: 700;
  }

  .count {
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.9em;
    color: var(--text-muted);
  }

  .search {
    flex: 0 1 200px;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 90px;
    color: var(--text-muted);
  }

  .search input {
    width: 100%;
    min-width: 0;
    height: 26px;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .search input:focus {
    border-color: var(--accent);
    outline: none;
  }

  .badge {
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .layouts {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }

  .layouts > .icon-button.active {
    background: var(--selected);
    color: var(--accent);
  }

  .menu {
    position: relative;
    margin-left: var(--space-1);
  }

  .popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 40;
    width: 240px;
    padding: var(--space-1) 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .current {
    padding: var(--space-2) var(--space-3) var(--space-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .popover button {
    display: block;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .popover button:hover {
    background: var(--hover);
  }

  .save-as {
    display: flex;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
  }

  .save-as input {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .popover .save-as button {
    width: auto;
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
  }

  .popover .danger {
    color: var(--danger);
  }

  .tip {
    margin: var(--space-1) var(--space-3) var(--space-2);
    font-size: 11px;
    color: var(--text-muted);
  }

  .area {
    flex: 1;
    min-height: 0;
  }

  @media (max-width: 700px) {
    .count {
      display: none;
    }

    /* The title gives way to the search box, but never the back button (#118). */
    .title {
      min-width: 36px;
    }

    .search {
      min-width: 56px;
    }

    /* The buttons sit mid-toolbar here, so hang the popovers from the toolbar's
       edge instead: one 300px wide from a button would be clipped off the left. */
    .menu {
      position: static;
    }

    .popover {
      right: var(--space-2);
      max-width: calc(100% - 2 * var(--space-2));
    }
  }
</style>
