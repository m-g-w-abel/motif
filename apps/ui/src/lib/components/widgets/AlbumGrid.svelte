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

  // Only the rows of cards in view are in the DOM. Columns follow the CSS
  // grid's auto-fill rule; the card height is measured once rendered.
  const GAP = 16;
  let width = $state(0);
  let viewHeight = $state(0);
  let scrollTop = $state(0);
  let cardHeight = $state(0);
  let list = $state<HTMLElement>();
  // `width` includes the 16px side padding.
  const columns = $derived(Math.max(1, Math.floor((width - 2 * GAP + GAP) / (cardSize + GAP))));
  const rowHeight = $derived((cardHeight || cardSize + 48) + GAP);
  const rowCount = $derived(Math.ceil(albums.length / columns));
  const firstRow = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - 1));
  const lastRow = $derived(Math.min(rowCount, Math.ceil((scrollTop + viewHeight) / rowHeight) + 1));
  const visible = $derived(albums.slice(firstRow * columns, lastRow * columns));

  $effect(() => {
    void visible;
    void width;
    const frame = requestAnimationFrame(() => (cardHeight = list?.querySelector('li')?.offsetHeight ?? 0));
    return () => cancelAnimationFrame(frame);
  });
</script>

<section class="album-grid">
  <div
    class="scroll"
    bind:clientHeight={viewHeight}
    onscroll={(e) => (scrollTop = (e.currentTarget as HTMLElement).scrollTop)}
  >
    <ul
      bind:this={list}
      bind:clientWidth={width}
      style:--card="{cardSize}px"
      style:padding-top="{GAP + firstRow * rowHeight}px"
      style:padding-bottom="{GAP + (rowCount - lastRow) * rowHeight}px"
    >
      {#each visible as album (album.key)}
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
    gap: 16px;
    margin: 0;
    padding: 0 16px;
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
