<script lang="ts">
  import { getApp } from '../../context';
  import { menu } from '../../contextMenu.svelte';
  import { formatTime, trackTitle } from '../../format';
  import { queueMenu } from '../../menus';
  import type { QueueItem } from '../../protocol/QueueItem';
  import { moveItem } from '../../reorder';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';
  import Scrollbar from '../Scrollbar.svelte';

  /** Collapsible queues show a header that folds the list away. */
  let { collapsible = false }: { collapsible?: boolean } = $props();

  /** A list context shows this many upcoming tracks until Show all; an album shows all of its own. */
  const PREVIEW = 10;

  const { engine } = getApp();
  const queue = $derived(engine.queue);
  const context = $derived(engine.player?.context ?? null);
  // shortcut: a context longer than the engine's window (200) only shows what the window holds; ask for more when someone needs it.
  const upcoming = $derived.by(() => {
    if (context?.next == null) return [];
    return engine.context.items.slice(Math.max(0, context.next - engine.context.offset));
  });
  let showAll = $state(false);
  const fromContext = $derived(context?.kind === 'album' || showAll ? upcoming : upcoming.slice(0, PREVIEW));
  const contextLeft = $derived(context?.next == null ? 0 : context.len - context.next);
  const remainingMs = $derived(
    [...queue, ...upcoming].reduce((n, item) => n + (item.track.duration_ms ?? 0), 0),
  );
  // Time is only known for the entries the client has.
  const meta = $derived(
    `${queue.length + contextLeft} tracks` + (upcoming.length === contextLeft ? ` · ${formatTime(remainingMs)} left` : ''),
  );

  const KEY = 'queue.collapsed';
  let collapsed = $state(load());
  let scroller = $state<HTMLElement>();

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

  function move(from: number, to: number) {
    const item = queue[from];
    // Moved here at once so the dropped row doesn't jump back until the engine replies.
    engine.queue = moveItem(queue, from, to);
    engine.run({ type: 'move_item', item_id: item.id, to });
  }

  function keydown(e: KeyboardEvent, item: QueueItem, queued: boolean) {
    if (queued && (e.key === 'Delete' || e.key === 'Backspace')) {
      e.preventDefault();
      engine.run({ type: 'remove_item', item_id: item.id });
    }
  }

  const open = $derived(!collapsible || !collapsed);
</script>

{#snippet entry(item: QueueItem, queued: boolean)}
  <div class="entry" class:failed={item.error}>
    <button
      class="play"
      title={item.error ?? undefined}
      onclick={() => engine.run({ type: 'play_item', item_id: item.id })}
      onkeydown={(e) => keydown(e, item, queued)}
      use:menu={() => queueMenu(engine, item, queued)}
    >
      <span class="name">{trackTitle(item.track)}</span>
      {#if item.track.artist}<span class="artist">{item.track.artist}</span>{/if}
      <span class="time">{item.track.duration_ms == null ? '' : formatTime(item.track.duration_ms)}</span>
    </button>
    {#if queued}
      <button
        class="remove"
        aria-label="Remove {trackTitle(item.track)} from the queue"
        title="Remove from queue"
        onclick={() => engine.run({ type: 'remove_item', item_id: item.id })}
      >
        <Icon name="close" size={12} />
      </button>
    {/if}
  </div>
{/snippet}

<section class="queue" class:open>
  <header>
    {#if collapsible}
      <button class="toggle" aria-expanded={open} onclick={toggle}>
        <Icon name={open ? 'chevron-down' : 'chevron-up'} size={14} />
        <span class="title">Up next</span>
        <span class="meta">{meta}</span>
      </button>
    {:else}
      <span class="title">Up next</span>
      <span class="meta">{meta}</span>
    {/if}
  </header>
  {#if open}
    <div class="body">
      <div class="list custom-scrollbar" bind:this={scroller}>
        {#if queue.length}
          <div class="heading">
            <h3>Next in queue</h3>
            <button class="link" onclick={() => engine.run({ type: 'clear_queue' })}>Clear</button>
          </div>
          <ReorderList items={queue} label="Queue" key={(item) => String(item.id)} onmove={move}>
            {#snippet row(item)}{@render entry(item, true)}{/snippet}
          </ReorderList>
        {/if}
        {#if fromContext.length}
          <div class="heading">
            <h3>Next from {context?.label ?? (context?.kind === 'album' ? 'this album' : 'this list')}</h3>
          </div>
          <ol aria-label="Next from {context?.label ?? 'the list'}">
            {#each fromContext as item (item.id)}
              <li>{@render entry(item, false)}</li>
            {/each}
          </ol>
          {#if fromContext.length < upcoming.length}
            <button class="link more" onclick={() => (showAll = true)}>Show all</button>
          {/if}
        {/if}
        {#if !queue.length && !fromContext.length}
          <p class="empty">Nothing up next. Double-click a track in the library to play it.</p>
        {/if}
      </div>
      <Scrollbar target={scroller} />
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

  .title,
  h3 {
    margin: 0;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  h3 {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-muted);
  }

  .meta {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.9em;
    color: var(--text-muted);
  }

  .body {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
  }

  .list {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 0 var(--space-3) var(--space-3);
  }

  .heading {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) 0 var(--space-1);
  }

  ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* Context rows line up with queue rows, which have a drag handle. */
  ol > li {
    padding-left: calc(18px + var(--space-1));
  }

  .entry {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
  }

  .play {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    height: 28px;
    padding: 0 var(--space-1);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
    line-height: 28px;
  }

  .play:hover {
    background: var(--hover);
  }

  .name {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.9em;
    color: var(--text-muted);
  }

  .time {
    margin-left: auto;
    flex: none;
    font-size: 0.9em;
    font-variant-numeric: tabular-nums;
    color: var(--text-muted);
  }

  .failed .name {
    color: var(--text-muted);
    text-decoration: line-through;
  }

  .remove {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
  }

  .remove:hover,
  .link:hover {
    color: var(--text);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    font-size: 0.9em;
    color: var(--text-muted);
    cursor: pointer;
  }

  .more {
    margin: var(--space-2) 0 0 calc(18px + var(--space-2));
  }

  .empty {
    margin: var(--space-4) 0;
    color: var(--text-muted);
  }
</style>
