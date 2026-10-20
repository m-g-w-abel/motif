<script lang="ts">
  import { getApp } from '../../context';
  import Icon from '../Icon.svelte';

  /** Previous / play-pause / next only; `compact` drops previous. */
  let { compact = false, size = 20 }: { compact?: boolean; size?: number } = $props();

  const { engine } = getApp();
  const playing = $derived(engine.player?.status === 'playing');
</script>

<div class="transport" style:--size="{size}px">
  {#if !compact}
    <button class="icon-button" aria-label="Previous" onclick={() => engine.run({ type: 'previous' })}>
      <Icon name="previous" {size} />
    </button>
  {/if}
  <button
    class="icon-button play"
    aria-label={playing ? 'Pause' : 'Play'}
    disabled={!engine.player?.queue.length}
    onclick={() => engine.run({ type: 'toggle_playback' })}
  >
    <Icon name={playing ? 'pause' : 'play'} size={size * 1.35} />
  </button>
  <button class="icon-button" aria-label="Next" onclick={() => engine.run({ type: 'next' })}>
    <Icon name="next" {size} />
  </button>
</div>

<style>
  .transport {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .icon-button {
    width: calc(var(--size) * 1.7);
    height: calc(var(--size) * 1.7);
    color: var(--text);
  }

  /* Play is marked by size alone, not a filled box. */
  .play {
    width: calc(var(--size) * 2);
    height: calc(var(--size) * 2);
  }

  .icon-button:hover:not(:disabled) {
    background: none;
    color: var(--accent);
  }
</style>
