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
  import { dragValue, gesture, mapped, slide } from '../../gestures';
  import { albumMenu, playTracks, queueTracks } from '../../menus';
  import type { TrackRef } from '../../protocol/TrackRef';
  import { viewSort } from '../../sort.svelte';
  import type { LibraryTrack } from '../../protocol/LibraryTrack';
  import type { TrackImage } from '../../protocol/TrackImage';
  import { formatTime } from '../../format';
  import { volumeSetter } from '../../volume';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';
  import Coverflow, { type Card } from '../player/Coverflow.svelte';

  /** In the main window a click expands the player; inside it, it doesn't. `playing` shows the playing track, not the selected one. */
  let {
    expands = true,
    playing = false,
    hidden = false,
  }: {
    expands?: boolean;
    playing?: boolean;
    /** Laid out elsewhere, unseen (the sheet's queue and lyrics steps): it takes its size again the moment it shows. */
    hidden?: boolean;
  } = $props();

  const { engine, sheet, library, settings, position } = getApp();
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
  // track cover, then the track's other images (back, booklet, disc…). Listed once per album, so
  // skipping tracks within it keeps the same cards and only slides.
  const albumId = $derived(!expands && !isDemo && track?.source === 'local' ? (track.album ? albumKey(track) : track.id) : null);
  /** Listed once the sheet opens; the cards stay while it closes. */
  const album = $derived(sheet.open ? albumId : null);
  /** The album the cards are for. */
  let listed = $state<string | null>(null);
  let covers = $state<{ key: string; titles: string[]; shared: boolean }[]>([]);
  let images = $state<TrackImage[]>([]);
  /** Each album track's cover, as listed with the cards (the details cache can be older). */
  let artOf = $state.raw(new Map<string, string>());
  $effect(() => {
    const a = album;
    const t = untrack(() => track);
    if (!a || !t) return;
    let current = true;
    const tracks: Promise<LibraryTrack[]> = t.album
      ? engine.query({ type: 'album_tracks', albums: [a], sort: [] }).then((r) => (r.type === 'tracks' ? r.tracks : []))
      : Promise.resolve([untrack(() => library.details(t))].filter((d) => !!d));
    // shortcut: extra embedded pictures come from the track the sheet opened on; others' extras show after reopening.
    Promise.all([tracks, library.images(t.id)]).then(
      ([tracks, found]) => {
        if (!current) return;
        const byKey = new Map<string, string[]>();
        for (const { track: x, art } of tracks) if (art) byKey.set(art, [...(byKey.get(art) ?? []), x.title ?? '']);
        covers = [...byKey].map(([key, titles]) => ({ key, titles, shared: titles.length === tracks.length }));
        images = found;
        artOf = new Map(tracks.flatMap((x) => (x.art ? [[x.track.id, x.art] as const] : [])));
        listed = a;
      },
      () => {},
    );
    return () => (current = false);
  });
  const own = $derived(track ? (artOf.get(track.id) ?? library.details(track)?.art) : undefined);
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
  // Side cards and controls show once the sheet is open and still: they fade in by class after it arrives.
  const settled = $derived(sheet.rest);
  // Until the list holds this track's cover (another album still loading), the track's own cover
  // alone, keyed by its art so it stays the same card (and the sheet's morph target) when the rest
  // arrive.
  const shownCards = $derived(
    albumId !== null && listed === albumId && cards.some((c) => c.key === own) ? cards : [{ key: own ?? '', kicker: '', title: '' }],
  );

  // The largest square that fits, measured rather than left to CSS so it
  // behaves the same in every webview.
  let width = $state(0);
  let height = $state(0);
  const PADDING = 12;
  const side = $derived(Math.max(0, Math.min(width, height) - 2 * PADDING));
  let box = $state<HTMLElement>();
  // Its size is otherwise observed a frame after a change; shown again, it must be that size at once, where the sheet's cover lands.
  $effect(() => {
    if (hidden || !box) return;
    width = box.clientWidth;
    height = box.clientHeight;
  });

  // In the sheet, touch gestures: swipe to skip, drag up or down for the volume (or seek), shown while it changes.
  let level = $state<{ value: number; text: string; icon: 'volume' | 'play' } | null>(null);
  let hide: ReturnType<typeof setTimeout> | undefined;
  let seekTimer: ReturnType<typeof setTimeout> | undefined;
  const show = (value: number, text: string, icon: 'volume' | 'play') => {
    level = { value, text, icon };
    clearTimeout(hide);
    hide = setTimeout(() => (level = null), 800);
  };
  function onPointerDown(e: PointerEvent) {
    if ((e.target as HTMLElement).closest('button:not(.shade)')) return;
    // The carousel's stage follows a swipe: the centre card's own transform is the sheet's morph.
    const stage = (e.currentTarget as HTMLElement).querySelector<HTMLElement>('.coverflow .stage');
    gesture(e, mapped(settings.current.gestures, 'artwork'), {
      next: () => engine.run({ type: 'next' }),
      previous: () => engine.run({ type: 'previous' }),
      volume: () => {
        const start = engine.player?.volume ?? 1;
        const set = volumeSetter(engine);
        return (dy) => {
          const v = dragValue(start, dy, Math.max(side, 200));
          set(v);
          show(v, `${Math.round(v * 100)}%`, 'volume');
        };
      },
      seek: () => {
        const duration = engine.current?.track.duration_ms ?? 0;
        if (!duration) return undefined;
        const start = position.ms / duration;
        return (dy) => {
          const ms = Math.round(dragValue(start, dy, Math.max(side, 200)) * duration);
          show(ms / duration, formatTime(ms), 'play');
          // Seeks once the finger rests, not on every move.
          clearTimeout(seekTimer);
          seekTimer = setTimeout(() => engine.run({ type: 'seek', position_ms: ms }), 150);
        };
      },
    }, stage ? slide(stage) : undefined);
  }
</script>

<!-- Touch gestures only; keys and the mouse have the player's controls. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section
  bind:this={box}
  class="artwork"
  bind:clientWidth={width}
  bind:clientHeight={height}
  onpointerdown={expands || !engine.can('playback') ? undefined : onPointerDown}
>
  {#if level !== null}
    <div class="level" aria-hidden="true">
      <Icon name={level.icon} size={16} />
      <span class="track"><span class="fill" style:transform="scaleX({level.value})"></span></span>
      {level.text}
    </div>
  {/if}
  {#if !expands}
    <!-- The sheet: always the carousel, so its centre card is the same box with or without the rest. -->
    {#if width > 0}
      <Coverflow cards={shownCards} bind:index {track} {width} {height} {items} {settled} />
    {/if}
  {:else if side > 0}
    <button class="frame" use:menu={items} title="Expand player" style:width="{side}px" onclick={() => sheet.expand()}>
      <Artwork {track} size={side} />
    </button>
  {/if}
</section>

<style>
  .artwork {
    position: relative;
    display: grid;
    place-items: center;
    height: 100%;
    min-height: 120px;
    background: var(--bg);
    overflow: hidden;
  }

  .level {
    position: absolute;
    top: var(--space-4);
    left: 50%;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    border-radius: 999px;
    background: var(--surface);
    color: var(--text);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    transform: translateX(-50%);
    pointer-events: none;
  }

  .level .track {
    width: 80px;
    height: 4px;
    border-radius: 2px;
    background: var(--divider);
    overflow: hidden;
  }

  .level .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
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
