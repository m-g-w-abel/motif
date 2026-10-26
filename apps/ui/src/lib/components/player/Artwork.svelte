<script lang="ts">
  import { getApp } from '../../context';
  import { placeholderArt } from '../../format';
  import type { TrackRef } from '../../protocol/TrackRef';
  import Icon from '../Icon.svelte';

  let {
    track,
    size,
    art,
    title = '',
  }: {
    track: TrackRef | null | undefined;
    /** Pixels; fills its container if absent. */
    size?: number;
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
</script>

<div
  class="art"
  style:width={size ? `${size}px` : undefined}
  style:height={size ? `${size}px` : undefined}
  style:background={track && !loading && (!id || failed === id) ? placeholderArt(track) : undefined}
>
  {#if id && failed !== id}
    <img
      src={library.artUrl(id, size ?? 256)}
      alt={title ? `Cover of ${title}` : ''}
      loading="lazy"
      decoding="async"
      onerror={() => (failed = id)}
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
