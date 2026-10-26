<script lang="ts">
  import type { TrackRef } from '../../protocol/TrackRef';
  import { placeholderArt } from '../../format';
  import Icon from '../Icon.svelte';

  let { track, size }: { track: TrackRef | null | undefined; size?: number } = $props();
</script>

<!-- Placeholder gradients until artwork extraction lands in phase 1/2. -->
<div
  class="art"
  style:width={size ? `${size}px` : undefined}
  style:height={size ? `${size}px` : undefined}
  style:background={track ? placeholderArt(track) : undefined}
>
  {#if !track}<Icon name="music" size={size ? Math.round(size * 0.4) : 48} />{/if}
</div>

<style>
  .art {
    flex: none;
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
  }
</style>
