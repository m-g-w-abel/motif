<script lang="ts">
  import { getApp } from '../../context';
  import { placeholderArt } from '../../format';
  import type { TrackRef } from '../../protocol/TrackRef';
  import Icon from '../Icon.svelte';
  import { albumOf, keptArt, type ShownArt } from './artwork';

  let {
    track,
    size,
    exact,
    art,
    title = '',
  }: {
    track: TrackRef | null | undefined;
    /** Pixels; fills its container if absent. */
    size?: number;
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
  // The decoded image on screen. It stays while the track is from the same album, even while
  // its own art id is unknown or decoding (no flicker); another album gets the placeholder.
  let shown = $state<ShownArt | null>(null);
  const kept = $derived(keptArt(shown, track, id));
  const visible = $derived(kept !== null && failed !== kept);
  const src = (a: string) => exact ? library.artUrl(a, exact, true) : library.artUrl(a, size ?? 256);
  $effect(() => {
    if (loading) return;
    if (!id) {
      shown = null;
      return;
    }
    const next = id;
    const album = albumOf(track) ?? '';
    const img = new Image();
    img.src = src(next);
    img.decode().then(
      () => (shown = { id: next, album }),
      () => {
        failed = next;
        shown = null;
      },
    );
    return () => (img.src = '');
  });
</script>

<div
  class="art"
  style:width={size ? `${size}px` : undefined}
  style:height={size ? `${size}px` : undefined}
  style:background={track && !loading && !visible ? placeholderArt(track) : undefined}
>
  {#if visible}
    <img
      src={src(kept!)}
      alt={title ? `Cover of ${title}` : ''}
      loading="lazy"
      decoding="async"
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
