<script lang="ts">
  import { getApp } from '../../context';
  import { formatTime } from '../../format';
  import Icon from '../Icon.svelte';
  import TrackList from './TrackList.svelte';

  /** Collapsible queues show a header that folds the list away. */
  let { collapsible = false }: { collapsible?: boolean } = $props();

  const { engine } = getApp();
  const queue = $derived(engine.player?.queue ?? []);
  const remainingMs = $derived(
    queue.slice((engine.player?.current ?? -1) + 1).reduce((n, item) => n + (item.track.duration_ms ?? 0), 0),
  );

  const KEY = 'queue.collapsed';
  let collapsed = $state(load());

  function load(): boolean {
    try {
      return localStorage.getItem(KEY) === 'true';
    } catch {
      return false;
    }
  }

  function toggle() {
    collapsed = !collapsed;
    try {
      localStorage.setItem(KEY, String(collapsed));
    } catch {
      // Just won't persist.
    }
  }

  const open = $derived(!collapsible || !collapsed);
</script>

<section class="queue" class:open>
  <header>
    {#if collapsible}
      <button class="toggle" aria-expanded={open} onclick={toggle}>
        <Icon name={open ? 'chevron-down' : 'chevron-up'} size={14} />
        <span class="title">Queue</span>
        <span class="meta">{queue.length} tracks · {formatTime(remainingMs)} left</span>
      </button>
    {:else}
      <span class="title">Queue</span>
      <span class="meta">{queue.length} tracks · {formatTime(remainingMs)} left</span>
    {/if}
    {#if queue.length}
      <button class="clear" onclick={() => engine.run({ type: 'clear_queue' })}>Clear</button>
    {/if}
  </header>
  {#if open}
    <div class="list">
      <TrackList source="queue" columns={['index', 'title', 'artist', 'duration']} />
    </div>
  {/if}
</section>

<style>
  .queue {
    display: flex;
    flex-direction: column;
    min-height: 0;
    background: var(--bg);
  }

  .queue.open {
    height: 100%;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 32px;
    padding: 0 var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  .toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    min-width: 0;
    height: 100%;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .title {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .meta {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.9em;
    color: var(--text-muted);
  }

  .clear {
    padding: 0;
    border: 0;
    background: none;
    font-size: 0.9em;
    color: var(--text-muted);
    cursor: pointer;
  }

  .clear:hover {
    color: var(--text);
  }

  .list {
    flex: 1;
    min-height: 0;
  }
</style>
