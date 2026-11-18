<script lang="ts">
  import { getApp } from '../../context';
  import { surface } from '../../motion/surface';
  import { pickActions, runPick, type Picking } from '../../picking';
  import Icon from '../Icon.svelte';

  // Phones: select mode's bar, over the player bar (`NowPlayingBar`), whose contents fade out under it: Done and
  // the count where the track is, the actions where the view buttons are. Its ⋮ opens the bar's menu (`PhoneMenu`).
  let { picking }: { picking: Picking } = $props();
  const { engine, playlists, views } = getApp();

  const actions = $derived(pickActions(picking, { playback: engine.can('playback'), queue: engine.can('queue'), playlist: engine.can('library') }));
  const open = $derived(views.menu === 'select');
  const choosing = $derived(views.menu === 'playlist');
  // Add to playlist asks which in the bar's menu, as ⋮ does; with nothing it can add, a tap does nothing.
  async function addToPlaylist() {
    if (choosing) return (views.menu = null);
    const shown = picking;
    if (!shown.count) return;
    try {
      if (playlists.canAdd(await shown.tracks()) && views.picking === shown) views.menu = 'playlist';
    } catch (e) {
      engine.error = (e as Error).message;
    }
  }
</script>

<div class="select-bar" role="toolbar" aria-label="Selected songs" transition:surface={{ enter: 'fade' }}>
  <div class="top">
    <button class="done" onclick={() => (views.selecting = false)}>Done</button>
    <span class="count" aria-live="polite">{picking.count} selected</span>
  </div>
  <div class="actions">
    {#each actions.bar as action (action.id)}
      <button
        class="icon-button"
        aria-label={action.label}
        title={action.label}
        class:active={action.id === 'playlist' && choosing}
        aria-expanded={action.id === 'playlist' ? choosing : undefined}
        onclick={() => (action.id === 'playlist' ? addToPlaylist() : runPick(action.id, picking, engine))}
      >
        <Icon name={action.icon} size={24} />
      </button>
    {/each}
    <button class="icon-button" class:active={open} aria-label="More actions" title="More actions" aria-expanded={open} onclick={() => (views.menu = open ? null : 'select')}>
      <Icon name="more" size={24} />
    </button>
  </div>
</div>

<style>
  /* Over the bar it stands in for; the bar's own background and border show through. */
  .select-bar {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }

  /* The track row's height (`NowPlayingBar`). */
  .top {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 60px;
    padding: 0 var(--space-3);
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
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* Spread as the view buttons are. */
  .actions {
    display: flex;
    justify-content: space-around;
    padding: var(--space-1) 0 var(--space-2);
  }

  .actions .icon-button {
    width: 56px;
    height: 40px;
    color: var(--text);
  }
</style>
