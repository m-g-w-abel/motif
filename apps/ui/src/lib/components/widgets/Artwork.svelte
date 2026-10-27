<script lang="ts">
  import { getApp, getViewState } from '../../context';
  import { albumKey } from '../../albums';
  import { menu } from '../../contextMenu.svelte';
  import { demoLibrary, isDemo } from '../../demo';
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { viewSort } from '../../sort.svelte';
  import Artwork from '../player/Artwork.svelte';

  /** In the main window a click expands the player; inside it, it doesn't. `playing` shows the playing track, not the selected one. */
  let { expands = true, playing = false }: { expands?: boolean; playing?: boolean } = $props();

  const { engine, sheet } = getApp();
  const view = getViewState();
  const track = $derived(playing ? engine.current?.track : (view.browse.selectedTrack ?? undefined));

  /** The shown track's album, in album order; the widget's menu is the album grid's. */
  async function albumTracks(t: TrackRef): Promise<TrackRef[]> {
    if (isDemo) return demoLibrary.filter((d) => albumKey(d) === albumKey(t));
    const result = await engine.query({ type: 'album_tracks', albums: [albumKey(t)], sort: viewSort(view) });
    return result.type === 'tracks' ? result.tracks.map((x) => x.track) : [];
  }
  const items = () => {
    const t = track;
    if (!t?.album) return [];
    return albumMenu(
      engine,
      { title: t.album, artist: t.artist ?? '' },
      () => albumTracks(t).then((tracks) => playTracks(engine, tracks)).catch((e: Error) => (engine.error = e.message)),
      () => albumTracks(t),
    );
  };

  // The largest square that fits, measured rather than left to CSS so it
  // behaves the same in every webview.
  let width = $state(0);
  let height = $state(0);
  const PADDING = 12;
  const side = $derived(Math.max(0, Math.min(width, height) - 2 * PADDING));
</script>

<section class="artwork" bind:clientWidth={width} bind:clientHeight={height}>
  {#if side > 0}
    {#if expands}
      <button class="frame" use:menu={items} title="Expand player" style:width="{side}px" onclick={() => sheet.expand()}>
        <Artwork {track} size={side} />
      </button>
    {:else}
      <div class="frame" use:menu={items} style:width="{side}px"><Artwork {track} size={side} /></div>
    {/if}
  {/if}
</section>

<style>
  .artwork {
    display: grid;
    place-items: center;
    height: 100%;
    min-height: 120px;
    background: var(--bg);
    overflow: hidden;
  }

  .frame {
    padding: 0;
    border: 0;
    background: none;
    line-height: 0;
  }

  button.frame {
    cursor: pointer;
  }

  .frame :global(.art) {
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.18);
  }
</style>
