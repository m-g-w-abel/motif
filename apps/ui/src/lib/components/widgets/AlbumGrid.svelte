<script lang="ts">
  import { albumKey, distinctAlbums } from '../../albums';
  import { getApp, getViewState } from '../../context';
  import { formatTime } from '../../format';
  import { trackListData } from '../../sources.svelte';
  import Artwork from '../player/Artwork.svelte';

  let { cardSize }: { cardSize: number } = $props();

  const { engine } = getApp();
  const view = getViewState();

  const data = $derived(trackListData(engine, view.source));
  const albums = $derived(distinctAlbums(data.tracks));
  const playingKey = $derived(engine.current ? albumKey(engine.current.track) : null);
</script>

<section class="album-grid">
  <div class="scroll">
    <ul style:--card="{cardSize}px">
      {#each albums as album (album.key)}
        <li>
          <button
            class="card"
            class:playing={album.key === playingKey}
            class:focused={album.key === view.browse.focusAlbum}
            title="{album.title} — {album.artist} · {album.indices.length} tracks · {formatTime(album.durationMs)}"
            onclick={() => (view.browse.focusAlbum = album.key)}
            ondblclick={() => data.playIndices(album.indices)}
          >
            <Artwork track={data.tracks[album.indices[0]]} />
            <span class="title">{album.title}</span>
            <span class="artist">{album.artist}</span>
          </button>
        </li>
      {/each}
    </ul>
  </div>
</section>

<style>
  .album-grid {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  ul {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(var(--card), 100%), 1fr));
    gap: var(--space-4);
    margin: 0;
    padding: var(--space-4);
    list-style: none;
  }

  .card {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: 0 0 var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: var(--surface);
    box-shadow: 0 1px 3px rgb(0 0 0 / 0.12);
    text-align: left;
    cursor: pointer;
    outline: 2px solid transparent;
    outline-offset: 1px;
  }

  .card :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  .card:hover {
    outline-color: var(--border);
  }

  .card.focused {
    outline-color: var(--text-muted);
  }

  .card.playing {
    outline-color: var(--accent);
  }

  .title,
  .artist {
    padding: 0 var(--space-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    margin-top: var(--space-2);
    font-weight: 500;
  }

  .artist {
    font-size: 0.88em;
    color: var(--text-muted);
  }
</style>
