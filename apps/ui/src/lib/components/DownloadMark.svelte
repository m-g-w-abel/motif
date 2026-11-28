<script lang="ts">
  import { getApp } from '../context';
  import type { Downloadable } from '../protocol/Downloadable';
  import Icon from './Icon.svelte';
  import Ring from './Ring.svelte';

  // A member's download state: cloud_download when not in the Downloaded group, a ring filling with the share
  // of its songs here while they download, cloud_done once all are. Redrawn only as the library changes (no animation).
  const { member, size = 18 }: { member: Downloadable; size?: number } = $props();
  const { downloads } = getApp();
  const state = $derived(downloads.state(member));
</script>

{#if state.kind === 'downloading'}
  <Ring share={state.share} {size} />
{:else}
  <Icon name={state.kind === 'done' ? 'cloud-done' : 'cloud-download'} {size} />
{/if}
