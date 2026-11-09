<script lang="ts">
  import { getApp } from '../../context';
  import { playTracks } from '../../menus';
  import Artwork from '../player/Artwork.svelte';

  const { engine, library } = getApp();
  const plays = library.recentPlays;
  $effect(() => plays.sync());

  const ROW = 48;

  // Only the rows in view are drawn.
  let scrollTop = $state(0);
  let height = $state(600);
  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 3));
  const last = $derived(Math.min(plays.total, Math.ceil((scrollTop + height) / ROW) + 3));
  const rows = $derived(Array.from({ length: Math.max(0, last - first) }, (_, k) => first + k));

  const when = (seconds: number) =>
    new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
</script>

<div class="recent" bind:clientHeight={height} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
  <ul class="songs" style:height="{plays.total * ROW}px">
    {#each rows as i (i)}
      {@const play = plays.get(i)}
      <li style:transform="translateY({i * ROW}px)">
        {#if play}
          <button class="song" onclick={() => playTracks(engine, [play.track.track])}>
            <Artwork track={play.track.track} art={play.track.art} size={32} />
            <span class="text">
              <span class="title">{play.track.track.title || 'Unknown title'}</span>
              <span class="artist">{play.track.track.artist || 'Unknown artist'} · {when(play.played_at)}</span>
            </span>
          </button>
        {/if}
      </li>
    {/each}
  </ul>
  {#if plays.loaded && !plays.total}
    <p class="empty">Nothing played yet. A song shows here once you have listened to half of it.</p>
  {/if}
</div>

<style>
  .recent {
    height: 100%;
    overflow-y: auto;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .songs {
    position: relative;
  }

  .songs li {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 48px;
  }

  .song {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    height: 100%;
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }

  .song:hover {
    background: var(--hover);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title,
  .artist {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .artist {
    font-size: 0.88em;
    color: var(--text-muted);
  }

  .empty {
    padding: var(--space-4) var(--space-3);
    color: var(--text-muted);
  }
</style>
