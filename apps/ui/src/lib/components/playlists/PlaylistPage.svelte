<script lang="ts">
  import { getApp } from '../../context';
  import { menu, type MenuItem } from '../../contextMenu.svelte';
  import { formatTime, trackTitle } from '../../format';
  import { queueTracks } from '../../menus';
  import type { PlaylistEntry } from '../../protocol/PlaylistEntry';
  import { moveItem } from '../../reorder';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';

  // shortcut: a plain list until the album/playlist page (#170) shows playlists like albums.
  let { id, name }: { id: string; name: string } = $props();
  const { engine, playlists } = getApp();
  let entries = $state<PlaylistEntry[]>([]);
  const editable = $derived(engine.can('library'));

  $effect(() => {
    if (!playlists.revision) return;
    playlists
      .entries(id)
      .then((e) => (entries = e))
      .catch(() => {});
  });

  function remove(entry: PlaylistEntry) {
    entries = entries.filter((e) => e !== entry);
    playlists.remove(id, [entry.item]);
  }

  // Moved here at once so the dropped row doesn't jump back until the engine replies.
  function move(from: number, to: number) {
    if (!editable) return;
    playlists.move(
      id,
      entries.map((e) => e.item),
      from,
      to,
    );
    entries = moveItem(entries, from, to);
  }

  function entryMenu(entry: PlaylistEntry, i: number): MenuItem[] {
    const items: MenuItem[] = [];
    if (engine.can('playback')) items.push({ label: 'Play', run: () => playlists.play(id, i) });
    if (engine.can('queue')) {
      items.push(
        { label: 'Queue next', run: () => queueTracks(engine, [entry.track.track], true) },
        { label: 'Add to queue', run: () => queueTracks(engine, [entry.track.track]) },
      );
    }
    if (editable) items.push({ label: 'Remove from playlist', run: () => remove(entry) });
    return items;
  }
</script>

<div class="list">
  {#if entries.length}
    <ReorderList items={entries} label={name} key={(entry) => entry.item} onmove={move}>
      {#snippet row(entry, i)}
        <div class="entry" class:missing={entry.track.missing}>
          <button class="play" onclick={() => playlists.play(id, i)} use:menu={() => entryMenu(entry, i)}>
            <span class="name">{trackTitle(entry.track.track)}</span>
            {#if entry.track.track.artist}<span class="artist">{entry.track.track.artist}</span>{/if}
            <span class="time">{entry.track.track.duration_ms == null ? '' : formatTime(entry.track.track.duration_ms)}</span>
          </button>
          {#if editable}
            <button
              class="remove"
              aria-label="Remove {trackTitle(entry.track.track)} from {name}"
              title="Remove from playlist"
              onclick={() => remove(entry)}
            >
              <Icon name="close" size={12} />
            </button>
          {/if}
        </div>
      {/snippet}
    </ReorderList>
  {:else}
    <p class="empty">No tracks yet. Add some from a track's menu: Add to playlist.</p>
  {/if}
</div>

<style>
  .list {
    padding: var(--space-2) var(--space-3);
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
    height: 32px;
    padding: 0 var(--space-1);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    text-align: left;
    cursor: pointer;
    line-height: 32px;
  }

  .play:hover {
    background: var(--hover);
  }

  .name,
  .artist {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    flex: 0 1 auto;
  }

  .artist {
    flex: 1 1 0;
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

  .missing .name {
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

  .empty {
    color: var(--text-muted);
  }
</style>
