<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp } from '../../context';
  import { placeholderArt } from '../../format';
  import type { TrackRef } from '../../protocol/TrackRef';
  import Icon from '../Icon.svelte';
  import { albumOf, decodedArt, keptArt, type ShownArt } from './artwork';

  let {
    track,
    size,
    drawn,
    exact,
    art,
    title = '',
  }: {
    track: TrackRef | null | undefined;
    /** Pixels; fills its container if absent. */
    size?: number;
    /** Pixels it is drawn at, when a transform shows it smaller than `size`. */
    drawn?: number;
    /** Image pixels to ask for whatever the pixel ratio, e.g. a size the engine has ready. */
    exact?: number;
    /** Artwork id. If not given, looked up in the library for `track`. */
    art?: string | null;
    /** For the image's description. */
    title?: string;
  } = $props();

  const { library } = getApp();
  const details = $derived(art !== undefined ? null : library.details(track ?? undefined));
  const id = $derived(art !== undefined ? art : (details?.art ?? null));
  // Still being looked up: neither the gradient nor an image yet.
  const loading = $derived(art === undefined && details === undefined);
  let failed = $state<string | null>(null);
  // The decoded image on screen. It stays while the track is from the same album, and while the
  // next art is looked up or decoding (no flicker). A cover decoded before (another screen) shows at once.
  const early = untrack(() => (id ? decodedArt.get(id) : undefined));
  let shown = $state<ShownArt | null>(untrack(() => (early && id ? { id, album: albumOf(track) ?? '' } : null)));
  /** The drawing of `shown` on screen: its own size, or a decoded one standing in until that decodes. */
  let shownUrl = $state(early?.url);
  // New art on its way: keep what's shown (or nothing) instead of the placeholder, which is only for
  // tracks known to have no art and images that fail.
  const waiting = $derived(loading || (id !== null && failed !== id));
  const kept = $derived(keptArt(shown, track, id, waiting));
  const visible = $derived(kept !== null && failed !== kept);
  const px = $derived(exact ?? drawn ?? size ?? 256);
  const src = (a: string) => exact ? library.artUrl(a, exact, true) : library.artUrl(a, drawn ?? size ?? 256);
  $effect(() => {
    if (loading) return;
    if (!id) {
      shown = null;
      return;
    }
    const next = id;
    const album = albumOf(track) ?? '';
    const url = src(next);
    const wanted = px;
    if (untrack(() => decodedArt.get(next))?.url === url) {
      shown = { id: next, album };
      shownUrl = url;
      return;
    }
    const img = new Image();
    img.src = url;
    // Superseded (e.g. the same art at a new size): its decode fails when cleared, which isn't a failure.
    let live = true;
    img.decode().then(
      () => {
        if (!live) return;
        decodedArt.add(next, url, wanted);
        shown = { id: next, album };
        shownUrl = url;
      },
      () => {
        if (!live) return;
        failed = next;
        shown = null;
      },
    );
    return () => {
      live = false;
      img.src = '';
    };
  });
</script>

<div
  class="art"
  style:width={size ? `${size}px` : undefined}
  style:height={size ? `${size}px` : undefined}
  style:background={track && !waiting && !visible ? placeholderArt(track) : undefined}
>
  {#if visible}
    <!-- Decoded already (above), so drawn in the frame it appears in, never blank for one. -->
    <img
      src={shownUrl ?? src(kept!)}
      alt={title ? `Cover of ${title}` : ''}
      decoding="sync"
      onerror={() => (failed = kept)}
    />
  {:else if !track}
    <Icon name="music" size={size ? Math.round(size * 0.4) : 48} />
  {/if}
</div>

<style>
  .art {
    flex: none;
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    overflow: hidden;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
</style>
