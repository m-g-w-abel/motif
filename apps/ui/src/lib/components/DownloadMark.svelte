<script lang="ts">
  import { getApp } from '../context';
  import type { Downloadable } from '../protocol/Downloadable';
  import Icon from './Icon.svelte';

  // A member's download state: cloud_download when not in the Downloaded group, a ring filling with the share
  // of its songs here while they download, cloud_done once all are. Redrawn only as the library changes (no animation).
  const { member, size = 18 }: { member: Downloadable; size?: number } = $props();
  const { downloads } = getApp();
  const state = $derived(downloads.state(member));
  // The ring's length at r = 8.
  const RING = 2 * Math.PI * 8;
</script>

{#if state.kind === 'downloading'}
  <svg class="ring" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
    <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2" opacity="0.3" />
    <circle
      cx="12"
      cy="12"
      r="8"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-dasharray="{state.share * RING} {RING}"
      transform="rotate(-90 12 12)"
    />
  </svg>
{:else}
  <Icon name={state.kind === 'done' ? 'cloud-done' : 'cloud-download'} {size} />
{/if}

<style>
  .ring {
    flex: none;
  }
</style>
