<script lang="ts">
  import { getApp } from '../../context';
  import { demoLibrary, isDemo } from '../../demo';
  import { libraryMenu } from '../../menus';
  import { CATEGORIES } from '../../views.svelte';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { canDecorate } from '../../windowChrome';
  import { contextMenu } from '../../contextMenu.svelte';
  import { engines } from '../../engine.svelte';
  import { switchTo } from '../../engines';
  import { HELP_URL, openUrl } from '../../openUrl';

  const { engine, library, settings, settingsView, views } = getApp();
  let scroller = $state<HTMLElement>();
  const statusLabel = { connecting: 'Connecting…', open: 'Connected', closed: 'Disconnected' };

  function openSwitcher(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    const others = engines.list.engines.filter((other) => other.id !== engines.list.current);
    const items = others.map((other) => ({ label: `Switch to ${other.name}`, run: () => engines.update((list) => switchTo(list, other.id)) }));
    // At the bottom of the screen: opens upwards from the button's top edge.
    contextMenu.show(items, box.left, box.top - 4, button);
  }

  function openMenu(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    contextMenu.show(libraryMenu(engine), box.left, box.bottom + 4, button);
  }

  const trackCount = () => (isDemo ? demoLibrary.length : (library.status?.tracks ?? 0));
  // The music area shows All Songs when only the Library home is open.
  const current = $derived(views.stack[1]?.kind ?? 'songs');
</script>

<!-- On desktop this is the Library home; on phones the home is a view (ViewScreen). -->
<nav class="nav" class:collapsed={settings.current.navCollapsed} aria-label="Library">
  {#if settings.current.ownDecorations && canDecorate}
    <!-- Also clears macOS's overlaid traffic lights. -->
    <div class="drag" data-tauri-drag-region></div>
  {/if}
  <div class="scroll" bind:this={scroller}>
    <div class="head">
      <h2>Library</h2>
      <button class="icon-button" aria-label="Library options" title="Library options" aria-haspopup="menu" onclick={openMenu}>
        <Icon name="more" size={14} />
      </button>
    </div>
    <ul class="sources">
      {#each CATEGORIES as category (category.kind)}
        <li>
          <button
            class:active={current === category.kind && !settingsView.group}
            aria-current={current === category.kind ? 'page' : undefined}
            onclick={() => {
              views.show({ kind: category.kind });
              settingsView.close();
            }}
          >
            <span class="round" style:--c={category.colour}><Icon name={category.icon} size={12} /></span>
            <span class="label">{category.label}</span>
            {#if category.kind === 'songs'}<span class="count">{trackCount()}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>

  </div>
  <Scrollbar target={scroller} />

  <!-- Pinned to the bottom of the screen. -->
  <div class="footer">
    {#if engines.list.engines.length > 1}
      <button
        class="connection switcher {engine.status}"
        aria-label="{engines.current.name}, {statusLabel[engine.status]}. Switch engine"
        title="{engines.current.name}: {statusLabel[engine.status]}"
        onclick={openSwitcher}
      >
        <span class="dot"></span><span class="engine">{engines.current.name}</span><Icon name="chevron-up" size={12} />
      </button>
    {:else}
      <span class="connection {engine.status}" title={statusLabel[engine.status]} role="status">
        <span class="dot"></span>{statusLabel[engine.status]}
      </span>
    {/if}
    <span class="actions">
      <button class="icon-button" aria-label="Changelog" title="Changelog" onclick={() => (views.changelog = true)}>
        <Icon name="list" size={16} />
      </button>
      <button class="icon-button" aria-label="Help (opens the docs online)" title="Help" onclick={() => openUrl(HELP_URL)}>
        <Icon name="info" size={16} />
      </button>
      <button
        class="icon-button"
        class:active={settingsView.group !== null}
        aria-label="Settings"
        title="Settings"
        onclick={() => settingsView.open()}
      >
        <Icon name="settings" size={16} />
      </button>
    </span>
  </div>
</nav>

<style>
  .nav {
    display: flex;
    flex-direction: column;
    width: var(--nav-width);
    height: 100%;
    border-right: 1px solid var(--divider);
    background: var(--surface);
    flex: none;
  }

  .drag {
    height: 36px;
    flex: none;
    border-bottom: 1px solid var(--divider);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .sources {
    margin: 0;
    padding: var(--space-1) 0;
    list-style: none;
  }

  .sources button {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    height: 32px;
    padding: 0 var(--space-3);
    border: 0;
    border-left: 2px solid transparent;
    background: none;
    color: var(--text-muted);
    text-align: left;
    cursor: pointer;
  }

  .sources button:hover {
    background: var(--hover);
    color: var(--text);
  }

  .sources button.active {
    border-left-color: var(--accent);
    background: var(--selected);
    color: var(--text);
    font-weight: 600;
  }

  .label {
    flex: 1;
  }

  .round {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--c);
    color: #fff;
    flex: none;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-right: var(--space-1);
  }

  .head h2 {
    margin-bottom: var(--space-2);
  }

  .count {
    font-size: 11px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  h2 {
    margin: var(--space-3) var(--space-3) var(--space-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    height: 40px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border-top: 1px solid var(--border);
    flex: none;
  }

  .actions {
    display: flex;
    flex: none;
  }

  .connection {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 11px;
    color: var(--text-muted);
  }

  .switcher {
    min-width: 0;
    height: 28px;
    padding: 0 var(--space-2);
    margin-left: calc(-1 * var(--space-2));
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    cursor: pointer;
  }

  .switcher:hover {
    background: var(--hover);
    color: var(--text);
  }

  .engine {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .connection.open .dot {
    background: var(--ok);
  }

  .connection.closed .dot {
    background: var(--danger);
  }

  /* Desktop: folded away with the toolbar's menu button (or Ctrl+B). */
  .nav.collapsed {
    display: none;
  }

  /* Phones show the Library home as a view instead. */
  @media (max-width: 700px) {
    .nav,
    .nav.collapsed {
      display: none;
    }
  }
</style>
