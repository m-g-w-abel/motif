<script lang="ts" module>
  import type { ImageKind } from '../../protocol/ImageKind';

  const KINDS: Record<ImageKind, string> = { front: 'Front', back: 'Back', booklet: 'Booklet', disc: 'Disc', artist: 'Artist' };
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, getViewState } from '../../context';
  import { albumKey } from '../../albums';
  import { menu } from '../../contextMenu.svelte';
  import { demoLibrary, isDemo } from '../../demo';
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { viewSort } from '../../sort.svelte';
  import type { LibraryTrack } from '../../protocol/LibraryTrack';
  import type { TrackImage } from '../../protocol/TrackImage';
  import Artwork from '../player/Artwork.svelte';
  import Coverflow, { type Card } from '../player/Coverflow.svelte';

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

  // In the sheet: every image of the playing album, listed when it opens (#18): each distinct
  // track cover, then the track's other images (back, booklet, disc…).
  const path = $derived(!expands && sheet.open && !isDemo && track?.source === 'local' ? track.id : null);
  let covers = $state<{ key: string; titles: string[]; shared: boolean }[]>([]);
  let images = $state<TrackImage[]>([]);
  $effect(() => {
    const p = path;
    const t = untrack(() => track);
    if (!p || !t) return;
    let current = true;
    const tracks: Promise<LibraryTrack[]> = t.album
      ? engine.query({ type: 'album_tracks', albums: [albumKey(t)], sort: [] }).then((r) => (r.type === 'tracks' ? r.tracks : []))
      : Promise.resolve([untrack(() => library.details(t))].filter((d) => !!d));
    Promise.all([tracks, library.images(p)]).then(
      ([tracks, found]) => {
        if (!current) return;
        const byKey = new Map<string, string[]>();
        for (const { track: x, art } of tracks) if (art) byKey.set(art, [...(byKey.get(art) ?? []), x.title ?? '']);
        covers = [...byKey].map(([key, titles]) => ({ key, titles, shared: titles.length === tracks.length }));
        images = found;
      },
      () => {},
    );
    return () => (current = false);
  });
  const own = $derived(track ? library.details(track)?.art : undefined);
  const cards = $derived.by(() => {
    const album = track?.album ?? '';
    // A cover all the album's tracks share is named after the album, else after its tracks.
    const list: Card[] = covers.map((c) => ({ key: c.key, kicker: 'Front', title: c.shared && album ? album : c.titles.join(' · ') }));
    for (const { key, kind } of images) {
      if (!list.some((c) => c.key === key)) list.push({ key, kicker: kind ? album : '', title: kind ? KINDS[kind] : '' });
    }
    return list;
  });
  // Lands on the playing track's cover when the track changes, and stays wherever it was paged to
  // until then.
  let index = $state(0);
  let landed: string | null = null;
  $effect(() => {
    const i = cards.findIndex((c) => c.key === own);
    const id = track?.id ?? null;
    if (i >= 0 && id !== landed) {
      landed = id;
      index = i;
    }
  });
  // Until the list holds this track's cover (another album still loading), the plain cover shows.
  const flow = $derived(path !== null && cards.length > 1 && cards.some((c) => c.key === own));

  // The largest square that fits, measured rather than left to CSS so it
  // behaves the same in every webview.
  let width = $state(0);
  let height = $state(0);
  const PADDING = 12;
  const side = $derived(Math.max(0, Math.min(width, height) - 2 * PADDING));
</script>

<section class="artwork" bind:clientWidth={width} bind:clientHeight={height}>
  {#if flow}
    <Coverflow {cards} bind:index {track} {width} {height} {items} settled={sheet.progress === 1} />
  {:else if side > 0}
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
