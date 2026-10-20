<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, setViewState, type ViewState } from '../../context';
  import { formatTime } from '../../format';
  import { installLayoutFiles } from '../../layout/install';
  import { trackListData } from '../../sources.svelte';
  import Icon from '../Icon.svelte';
  import LayoutNode from './LayoutNode.svelte';

  let { onEditLayout }: { onEditLayout: () => void } = $props();

  const { engine, layout, drawer, settings } = getApp();

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

  const data = $derived(trackListData(engine, view.source));
  const totalMs = $derived(data.tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0));

  let menuOpen = $state(false);
  let menu = $state<HTMLElement>();
  let fileInput = $state<HTMLInputElement>();

  function onWindowPointer(e: PointerEvent) {
    if (menuOpen && menu && !menu.contains(e.target as Node)) menuOpen = false;
  }
</script>

<svelte:window
  onpointerdown={onWindowPointer}
  onkeydown={(e) => {
    if (e.key === 'Escape') menuOpen = false;
    if (e.key === 'b' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      toggleNav();
    }
  }}
/>

<section class="content" aria-label={data.title}>
  <header class="toolbar">
    <div class="title">
      <button
        class="icon-button menu-button"
        aria-label="Toggle navigation"
        title="Toggle navigation (Ctrl+B)"
        onclick={toggleNav}
      >
        <Icon name="list" size={18} />
      </button>
      <h1>{data.title}</h1>
      <span class="count">{data.tracks.length} tracks · {formatTime(totalMs)}</span>
      <span class="badge" title="Real library scanning arrives in phase 1">Demo data</span>
    </div>

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
          onclick={() => (menuOpen = !menuOpen)}
        >
          <Icon name="more" size={16} />
        </button>
        {#if menuOpen}
          <div class="popover" role="menu">
            <div class="current">{layout.layout.name}</div>
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
  </header>

  <div class="area">
    <LayoutNode node={layout.layout.root} />
  </div>

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
    height: 36px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex: none;
  }

  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
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
      overflow-y: auto;
    }

    .count {
      display: none;
    }
  }
</style>
