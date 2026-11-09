<script lang="ts">
  import { getApp } from '../../context';
  import { canDiscover } from '../../discover';
  import { engines } from '../../engine.svelte';
  import { switchTo } from '../../engines';
  import Icon from '../Icon.svelte';

  // The phone bar's menu, grown up out of the bar (`NowPlayingBar`): the
  // connection, which opens the engine switcher in its place, then Settings.
  const { engine, settingsView, views } = getApp();
  const statusLabel = { connecting: 'connecting…', open: 'connected', closed: 'disconnected' };

  function settings() {
    views.menu = null;
    settingsView.open();
  }

  // Pairing this phone with another computer: scan its QR (Add an engine), else Settings → Devices.
  function pair() {
    views.menu = null;
    settingsView.open('devices');
    settingsView.addingEngine = canDiscover();
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (views.menu = null)} />

<div class="menu" role="dialog" aria-label="Menu">
  <h2>motif</h2>
  <button
    class="connection {engine.status}"
    aria-expanded={views.menu === 'engines'}
    onclick={() => (views.menu = views.menu === 'engines' ? 'main' : 'engines')}
  >
    <span class="dot"></span>{engine.unpaired ? 'not paired' : statusLabel[engine.status]}
    <span class="chip">{engines.current.name}</span>
  </button>

  {#if views.menu === 'engines'}
    {#each engines.list.engines as saved (saved.id)}
      {@const current = saved.id === engines.list.current}
      <button
        class="item"
        aria-current={current}
        onclick={() => (current ? (views.menu = 'main') : engines.update((list) => switchTo(list, saved.id)))}
      >
        <span class="connection {current ? engine.status : ''}"><span class="dot"></span></span>
        <span class="name">{saved.name}</span>
        {#if current}<small>in control</small>{/if}
      </button>
    {/each}
    <button class="item" onclick={pair}><Icon name="plus" size={20} />Pair a device</button>
  {:else}
    <button class="item" onclick={settings}><Icon name="settings" size={20} />Settings</button>
    <!-- Changelog and Help join here with the changelog ticket. -->
  {/if}
</div>

<style>
  .menu {
    padding: var(--space-4) var(--space-3) var(--space-2);
  }

  h2 {
    margin: 0;
    text-align: center;
    font-size: 26px;
  }

  .connection {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  button.connection {
    justify-content: center;
    width: 100%;
    margin: var(--space-2) 0 var(--space-3);
    padding: var(--space-1);
    border: 0;
    background: none;
    color: var(--text);
    font-weight: 600;
    cursor: pointer;
  }

  .chip {
    padding: 2px var(--space-2);
    border-radius: 999px;
    background: var(--hover);
    font-size: 0.9em;
  }

  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .connection.open .dot {
    background: var(--ok);
  }

  .connection.closed .dot {
    background: var(--danger);
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: 100%;
    padding: var(--space-3) var(--space-2);
    border: 0;
    border-radius: var(--radius-lg);
    background: none;
    color: var(--text);
    font-size: 1.15em;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }

  .item:hover {
    background: var(--hover);
  }

  .item .connection {
    width: 20px;
    justify-content: center;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    font-size: 0.75em;
    font-weight: 400;
    color: var(--text-muted);
  }
</style>
