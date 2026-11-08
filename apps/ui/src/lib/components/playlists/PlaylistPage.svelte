<script lang="ts">
  import { getApp } from '../../context';
  import { contextMenu, menu, type MenuItem } from '../../contextMenu.svelte';
  import { formatTime, trackTitle } from '../../format';
  import { queueTracks } from '../../menus';
  import type { PlaylistEntry } from '../../protocol/PlaylistEntry';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';
  import Scrollbar from '../Scrollbar.svelte';

  // shortcut: a plain page until the album/playlist page (#170) shows playlists like albums.
  const { engine, playlists } = getApp();
  const playlist = $derived(playlists.current);
  let scroller = $state<HTMLElement>();
  $effect(() => playlists.sync());

  const editable = $derived(engine.can('library'));

  function entryMenu(entry: PlaylistEntry, i: number): MenuItem[] {
    const id = playlist?.id;
    if (!id) return [];
    const items: MenuItem[] = [];
    if (engine.can('playback')) items.push({ label: 'Play', run: () => playlists.play(id, i) });
    if (engine.can('queue')) {
      items.push(
        { label: 'Queue next', run: () => queueTracks(engine, [entry.track.track], true) },
        { label: 'Add to queue', run: () => queueTracks(engine, [entry.track.track]) },
      );
    }
    if (editable) items.push({ label: 'Remove from playlist', run: () => playlists.remove(id, [entry.item]) });
    return items;
  }

  function openMenu(e: MouseEvent) {
    if (!playlist) return;
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    contextMenu.show(playlists.menu(playlist), box.left, box.bottom + 4, button);
  }
</script>

<section class="playlist">
  <header>
    <button class="icon-button" aria-label="Back to the library" title="Back" onclick={() => playlists.show(null)}>
      <Icon name="chevron-left" size={16} />
    </button>
    <h2>{playlist?.name ?? ''}</h2>
    {#if playlist}
      <span class="meta">{playlist.tracks} tracks · {formatTime(playlist.duration_ms)}</span>
      <button class="icon-button" aria-label="{playlist.name} menu" title="More" aria-haspopup="menu" onclick={openMenu}>
        <Icon name="more" size={16} />
      </button>
    {/if}
  </header>
  <div class="body">
    <div class="list custom-scrollbar" bind:this={scroller}>
      {#if playlist && playlists.entries.length}
        <ReorderList
          items={playlists.entries}
          label={playlist.name}
          key={(entry) => entry.item}
          onmove={(from, to) => editable && playlists.move(playlist.id, from, to)}
        >
          {#snippet row(entry, i)}
            <div class="entry" class:missing={entry.track.missing}>
              <button class="play" onclick={() => playlists.play(playlist.id, i)} use:menu={() => entryMenu(entry, i)}>
                <span class="name">{trackTitle(entry.track.track)}</span>
                {#if entry.track.track.artist}<span class="artist">{entry.track.track.artist}</span>{/if}
                <span class="time">{entry.track.track.duration_ms == null ? '' : formatTime(entry.track.track.duration_ms)}</span>
              </button>
              {#if editable}
                <button
                  class="remove"
                  aria-label="Remove {trackTitle(entry.track.track)} from {playlist.name}"
                  title="Remove from playlist"
                  onclick={() => playlists.remove(playlist.id, [entry.item])}
                >
                  <Icon name="close" size={12} />
                </button>
              {/if}
            </div>
          {/snippet}
        </ReorderList>
      {:else if playlist}
        <p class="empty">No tracks yet. Right-click tracks in the library and choose Add to playlist.</p>
      {/if}
    </div>
    <Scrollbar target={scroller} />
  </div>
</section>

<style>
  .playlist {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  h2 {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 1.1em;
  }

  .meta {
    flex: 1;
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
