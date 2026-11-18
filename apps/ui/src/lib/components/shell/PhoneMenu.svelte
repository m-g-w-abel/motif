<script lang="ts">
  import { getApp } from '../../context';
  import { canDiscover } from '../../discover';
  import { engines } from '../../engine.svelte';
  import { switchTo } from '../../engines';
  import { HELP_URL, openUrl } from '../../openUrl';
  import type { MenuItem } from '../../contextMenu.svelte';
  import { pickActions, runPick, type PickAction, type Picking } from '../../picking';
  import Icon from '../Icon.svelte';

  // The phone bar's menu, grown up out of the bar (`NowPlayingBar`): the
  // connection, which opens the engine switcher in its place, then Settings, Changelog and Help.
  // In select mode, the select bar's ⋮ opens it with the actions that don't fit on the bar,
  // and its Add to playlist with the playlists to add to (`Playlists.choices`).
  const { engine, playlists, settingsView, views } = getApp();
  const statusLabel = { connecting: 'connecting…', open: 'connected', closed: 'disconnected' };

  // One of the select bar's menus, which keeps showing what it showed while it closes.
  let kept: { mode: 'select' | 'playlist'; picking: Picking } | null = null;
  const shown = $derived.by(() => {
    const mode = views.menu;
    if ((mode === 'select' || mode === 'playlist') && views.picking) kept = { mode, picking: views.picking };
    else if (mode !== null) kept = null;
    return kept;
  });
  const more = $derived(shown?.mode === 'select' ? pickActions(shown.picking, { playback: false, queue: engine.can('queue'), playlist: false }).more : null);
  const choices = $derived(shown?.mode === 'playlist' ? playlists.choices(shown.picking.tracks(), shown.picking.done) : null);

  function pick(id: Exclude<PickAction['id'], 'playlist'>) {
    const on = shown?.picking;
    views.menu = null;
    if (on) runPick(id, on, engine);
  }

  function choose(choice: MenuItem) {
    views.menu = null;
    choice.run();
  }

  function settings() {
    views.menu = null;
    settingsView.open();
  }

  function changelog() {
    views.menu = null;
    views.changelog = true;
  }

  function help() {
    views.menu = null;
    void openUrl(HELP_URL);
  }

  // Pairing this phone with another computer: scan its QR (Add an engine), else Settings → Devices.
  function pair() {
    views.menu = null;
    settingsView.open('devices');
    settingsView.addingEngine = canDiscover();
  }
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && (views.menu = null)} />

<div class="menu" class:choices role="dialog" aria-label={more ? 'Selected songs' : choices ? 'Add to playlist' : 'Menu'}>
  {#if more}
    {#each more as action (action.id)}
      <button class="item" class:danger={action.id === 'delete'} onclick={() => pick(action.id)}>{action.label}</button>
    {/each}
  {:else if choices}
    <!-- By position: playlists may share a name. -->
    {#each choices as choice, i (i)}
      <button class="item" onclick={() => choose(choice)}>{choice.label}</button>
    {/each}
  {:else}
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
      <button class="item" onclick={changelog}><Icon name="list" size={20} />Changelog</button>
      <button class="item" onclick={help}><Icon name="info" size={20} />Help</button>
    {/if}
  {/if}
</div>

<style>
  .menu {
    padding: var(--space-4) var(--space-3) var(--space-2);
  }

  /* Many playlists scroll, under a finger too (the bar it grows from takes touches for its drags). */
  .menu.choices {
    max-height: 60vh;
    overflow-y: auto;
    touch-action: pan-y;
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

  .item.danger {
    color: var(--danger);
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
