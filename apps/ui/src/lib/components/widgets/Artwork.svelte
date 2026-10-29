<script lang="ts">
  import { getApp, getViewState } from '../../context';
  import { albumKey } from '../../albums';
  import { menu } from '../../contextMenu.svelte';
  import { demoLibrary, isDemo } from '../../demo';
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { viewSort } from '../../sort.svelte';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';

  /** In the main window a click expands the player; inside it, it doesn't. `playing` shows the playing track, not the selected one. */
  let { expands = true, playing = false }: { expands?: boolean; playing?: boolean } = $props();

  const { engine, sheet, library } = getApp();
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

  // In the sheet: the track's other images (back, booklet, disc…), listed when it opens. Page 0 is
  // the track's own art, shown as everywhere else.
  const path = $derived(!expands && sheet.open && !isDemo && track?.source === 'local' ? track.id : null);
  let images = $state<string[]>([]);
  let page = $state(0);
  $effect(() => {
    const p = path;
    images = [];
    page = 0;
    if (!p) return;
    let current = true;
    library.images(p).then(
      (keys) => current && (images = keys),
      () => {},
    );
    return () => (current = false);
  });
  const own = $derived(track ? library.details(track)?.art : undefined);
  const pages = $derived([undefined, ...images.filter((k) => k !== own)]);
  const turn = (by: number) => (page = (page + by + pages.length) % pages.length);

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
      <div class="frame" use:menu={items} style:width="{side}px">
        <Artwork {track} size={side} art={pages[page]} />
        {#if pages.length > 1 && sheet.progress === 1}
          <button class="turn previous" aria-label="Previous image" onclick={() => turn(-1)}>
            <Icon name="chevron-left" />
          </button>
          <button class="turn next" aria-label="Next image" onclick={() => turn(1)}>
            <Icon name="chevron-right" />
          </button>
          <span class="count">{page + 1} / {pages.length}</span>
        {/if}
      </div>
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
    position: relative;
    padding: 0;
    border: 0;
    background: none;
    line-height: 0;
  }

  button.frame {
    cursor: pointer;
  }

  /* Paging through the sheet's images, over the artwork. */
  .turn,
  .count {
    position: absolute;
    border: 0;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.5);
    color: #fff;
    line-height: 1;
  }

  .turn {
    top: 50%;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    margin-top: -18px;
    padding: 0;
    cursor: pointer;
  }

  .turn:hover {
    background: rgb(0 0 0 / 0.7);
  }

  .previous {
    left: 8px;
  }

  .next {
    right: 8px;
  }

  .count {
    bottom: 8px;
    left: 50%;
    transform: translateX(-50%);
    padding: 4px 8px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .frame :global(.art) {
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.18);
  }
</style>
