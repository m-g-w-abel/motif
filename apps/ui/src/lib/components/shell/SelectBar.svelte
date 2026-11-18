<script lang="ts">
  import { getApp } from '../../context';
  import { contextMenu } from '../../contextMenu.svelte';
  import { playTracks, queueTracks } from '../../menus';
  import { surface } from '../../motion/surface';
  import { pickActions, type PickAction, type Picking } from '../../picking';
  import Icon from '../Icon.svelte';

  // Phones: select mode's bar, in the player bar's place and shape (`NowPlayingBar`), which morphs into it and back.
  let { picking }: { picking: Picking } = $props();
  const { engine, playlists, sheet, views } = getApp();

  const actions = $derived(
    pickActions(picking, { playback: engine.can('playback'), queue: engine.can('queue'), playlist: playlists.canAdd(picking.tracks()) }),
  );

  function run(id: PickAction['id'], button: HTMLElement) {
    const tracks = picking.tracks();
    const box = button.getBoundingClientRect();
    if (id === 'play') playTracks(engine, tracks);
    else if (id === 'queue') queueTracks(engine, tracks);
    else if (id === 'next') queueTracks(engine, tracks, true);
    else if (id === 'playlist') playlists.pick(tracks, box.left, box.top - 8, button);
    else if (id === 'all') picking.toggleAll();
    else picking.remove?.();
  }

  function more(e: MouseEvent) {
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    const items = actions.more.map((a) => ({ label: a.label, disabled: a.disabled, run: () => run(a.id, button) }));
    contextMenu.show(items, box.right, box.top - 8, button);
  }
</script>

<div class="select-bar" role="toolbar" aria-label="Selected songs" transition:surface={{ enter: 'fade', from: sheet.origin ?? undefined }}>
  <button class="done" onclick={() => (views.selecting = false)}>Done</button>
  <span class="count" aria-live="polite">{picking.count} selected</span>
  <span class="grow"></span>
  {#each actions.bar as action (action.id)}
    <button
      class="icon-button"
      aria-label={action.label}
      title={action.label}
      aria-haspopup={action.id === 'playlist' ? 'menu' : undefined}
      disabled={action.disabled}
      onclick={(e) => run(action.id, e.currentTarget)}
    >
      <Icon name={action.icon} size={22} />
    </button>
  {/each}
  <button class="icon-button" aria-label="More actions" title="More actions" aria-haspopup="menu" onclick={more}>
    <Icon name="more" size={22} />
  </button>
</div>

<style>
  /* Covers the bar it stands in for, border and corners included. */
  .select-bar {
    position: absolute;
    inset: -1px;
    z-index: 2;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0 var(--space-2) 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
  }

  .done {
    height: 36px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
    cursor: pointer;
  }

  .count {
    margin-left: var(--space-2);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .grow {
    flex: 1;
  }

  .icon-button {
    width: 44px;
    height: 44px;
    color: var(--text);
  }
</style>
