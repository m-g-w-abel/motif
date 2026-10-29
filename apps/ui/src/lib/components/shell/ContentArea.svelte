<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { isDemo } from '../../demo';
  import { formatTime } from '../../format';
  import { installLayoutFiles } from '../../layout/install';
  import { toasts } from '../../toast.svelte';
  import type { SortKey } from '../../protocol/SortKey';
  import type { SortRule } from '../../protocol/SortRule';
  import { moveItem } from '../../reorder';
  import { DEFAULT_RULES, isDefault, SORT_KEYS, sortRules, viewSort } from '../../sort.svelte';
  import type { LibraryTracks, Shown } from '../../library.svelte';
  import { trackListData } from '../../sources.svelte';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { rootPath } from '../../layout/overlay';
  import LayoutNode from './LayoutNode.svelte';
  import { canDecorate } from '../../windowChrome';
  import WindowButtons from './WindowButtons.svelte';

  let { onEditLayout }: { onEditLayout: () => void } = $props();

  const { engine, library, layout, drawer, settings } = getApp();

  /** Phones open the drawer; desktops fold the left panel away or back. */
  function toggleNav() {
    if (window.innerWidth <= 700) drawer.toggle();
    else settings.set('navCollapsed', !settings.current.navCollapsed);
  }

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
    if (sortOpen && sortMenu && !sortMenu.contains(e.target as Node)) sortOpen = false;
  }

  // The view's saved sort rules, tier by tier. Editing them drops a one-off
  // column sort so the edit shows.
  let sortOpen = $state(false);
  let sortMenu = $state<HTMLElement>();
  const rules = $derived(sortRules.rules[view.source]);
  const sortKeys = Object.keys(SORT_KEYS) as SortKey[];

  function setRules(next: SortRule[]) {
    view.browse.oneOff = null;
    sortRules.set(view.source, next);
  }
  const change = (i: number, rule: Partial<SortRule>) => setRules(rules.map((r, j) => (j === i ? { ...r, ...rule } : r)));
  const remove = (i: number) => setRules(rules.filter((_, j) => j !== i));
  const add = () =>
    setRules([...rules, { key: sortKeys.find((k) => !rules.some((r) => r.key === k)) ?? 'title', descending: false }]);
</script>

<svelte:window
  onpointerdown={onWindowPointer}
  onkeydown={(e) => {
    if (e.key === 'Escape') menuOpen = sortOpen = false;
    if (e.key === 'b' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      toggleNav();
    }
  }}
/>

<section class="content" aria-label={data.title}>
  <header class="toolbar" {...drag}>
    <div class="title" {...drag}>
      <button
        class="icon-button menu-button"
        aria-label="Toggle navigation"
        title="Toggle navigation (Ctrl+B)"
        onclick={toggleNav}
      >
        <Icon name="list" size={18} />
      </button>
      <h1 {...drag}>{data.title}</h1>
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

    <div class="menu" bind:this={sortMenu}>
      <button
        class="icon-button"
        class:active={!isDefault(rules) || view.browse.oneOff}
        aria-haspopup="dialog"
        aria-expanded={sortOpen}
        aria-label="Sort order"
        title="Sort order"
        onclick={() => (sortOpen = !sortOpen)}
      >
        <Icon name="sort" size={16} />
      </button>
      {#if sortOpen}
        <div class="popover sorting" role="dialog" aria-label="Sort order">
          <div class="current">Sort by</div>
          {#if view.browse.oneOff}
            <p class="tip">
              Sorted by {SORT_KEYS[view.browse.oneOff.key].toLowerCase()} from a column header for now.
              <button class="link" onclick={() => (view.browse.oneOff = null)}>Use these rules</button>
            </p>
          {/if}
          <div class="tiers">
            <ReorderList items={rules} label="Sort tiers" onmove={(from, to) => setRules(moveItem(rules, from, to))}>
              {#snippet row(rule, i)}
                <div class="tier">
                  <select
                    aria-label="Sort tier {i + 1}"
                    value={rule.key}
                    onchange={(e) => change(i, { key: e.currentTarget.value as SortKey })}
                  >
                    {#each sortKeys as key (key)}
                      <option value={key}>{SORT_KEYS[key]}</option>
                    {/each}
                  </select>
                  <button
                    class="direction"
                    title={rule.descending ? 'Descending: click for ascending' : 'Ascending: click for descending'}
                    onclick={() => change(i, { descending: !rule.descending })}
                  >
                    {rule.descending ? 'Desc' : 'Asc'}
                  </button>
                  <button
                    class="icon"
                    aria-label="Remove tier"
                    title="Remove tier"
                    disabled={rules.length === 1}
                    onclick={() => remove(i)}
                  >
                    <Icon name="close" size={12} />
                  </button>
                </div>
              {/snippet}
            </ReorderList>
          </div>
          {#if rules.length < sortKeys.length}<button onclick={add}>Add a tier</button>{/if}
          {#if !isDefault(rules)}<button onclick={() => setRules([...DEFAULT_RULES])}>Reset to default</button>{/if}
          <p class="tip">Click a column header to sort by it once; this order stays saved.</p>
        </div>
      {/if}
    </div>

    {#if layout.arranging}
      <button class="button primary" onclick={() => (layout.arranging = false)}>Done</button>
    {/if}
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
            <p class="tip">Or drop a .kdl file onto the window.</p>
          </div>
        {/if}
      </div>
    </div>

    <WindowButtons />
  </header>

  <div class="area" bind:this={area}>
    <LayoutNode node={layout.shown} path={rootPath(layout.shown)} />
  </div>
  <Scrollbar target={area} />

  <input
    bind:this={fileInput}
    type="file"
    accept=".kdl"
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
       below; the z-index lifts them (and any later toolbar menu) over the area. */
    overflow-x: clip;
    position: relative;
    z-index: 1;
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

  .menu-button {
    align-self: center;
    margin-left: calc(-1 * var(--space-2));
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

  .sorting {
    width: 300px;
  }

  .tiers {
    padding: 0 var(--space-3) var(--space-1);
  }

  .tier {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px 0;
  }

  .sorting select {
    flex: 1;
    min-width: 0;
    height: 24px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .popover .tier button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: var(--radius-sm);
    text-align: center;
    font-size: 10px;
  }

  .popover .tier .direction {
    width: 40px;
    font-size: 11px;
  }

  .popover .tier button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .popover .link {
    display: inline;
    width: auto;
    padding: 0;
    color: var(--accent);
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
    .area {
      /* Holds the bars of the scroll areas inside it, so they scroll with it. */
      position: relative;
      overflow-y: auto;
    }

    .count {
      display: none;
    }

  }
</style>
