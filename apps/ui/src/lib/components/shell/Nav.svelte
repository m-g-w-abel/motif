<script lang="ts">
  import { getApp } from '../../context';
  import { demoLibrary, isDemo } from '../../demo';
  import type { Source } from '../../layout/store.svelte';
  import Icon, { type IconName } from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import { canDecorate } from '../../windowChrome';
  import { contextMenu } from '../../contextMenu.svelte';
  import { engines } from '../../engine.svelte';
  import { switchTo } from '../../engines';

  const { engine, library, layout, drawer, settings, settingsView } = getApp();
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

  const sources: { id: Source; label: string; icon: IconName; count: () => number }[] = [
    { id: 'library', label: 'Library', icon: 'library', count: () => (isDemo ? demoLibrary.length : (library.status?.tracks ?? 0)) },
  ];
</script>

<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
{#if drawer.open}<div class="scrim" onclick={() => drawer.close()}></div>{/if}

<nav class="nav" class:open={drawer.open} class:collapsed={settings.current.navCollapsed} aria-label="Browse">
  {#if settings.current.ownDecorations && canDecorate}
    <!-- Also clears macOS's overlaid traffic lights. -->
    <div class="drag" data-tauri-drag-region></div>
  {/if}
  <div class="scroll" bind:this={scroller}>
    <ul class="sources">
      {#each sources as source (source.id)}
        <li>
          <button
            class:active={layout.source === source.id && !settingsView.group}
            aria-current={layout.source === source.id ? 'page' : undefined}
            onclick={() => {
              layout.browse(source.id);
              settingsView.close();
              drawer.close();
            }}
          >
            <Icon name={source.icon} size={16} />
            <span class="label">{source.label}</span>
            <span class="count">{source.count()}</span>
          </button>
        </li>
      {/each}
    </ul>

    <h2>Playlists</h2>
    <p class="note">Saved playlists arrive in a later phase.</p>
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
    <button
      class="icon-button"
      class:active={settingsView.group !== null}
      aria-label="Settings"
      title="Settings"
      onclick={() => {
        settingsView.open();
        drawer.close();
      }}
    >
      <Icon name="settings" size={16} />
    </button>
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

  .note {
    margin: 0 var(--space-3);
    font-size: 11px;
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

  .scrim {
    display: none;
  }

  /* Phones: an off-screen drawer instead of a column. */
  @media (max-width: 700px) {
    .nav,
    .nav.collapsed {
      display: flex;
      position: fixed;
      top: 0;
      bottom: 0;
      left: 0;
      z-index: 46;
      width: min(280px, 85vw);
      padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom);
      box-shadow: var(--shadow);
      transform: translateX(-100%);
      transition: transform 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
      touch-action: pan-y;
    }

    .nav.open {
      transform: none;
    }

    .scrim {
      display: block;
      position: fixed;
      inset: 0;
      z-index: 46;
      background: rgb(0 0 0 / 0.45);
    }
  }
</style>
