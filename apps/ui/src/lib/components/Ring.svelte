<script lang="ts">
  // A ring filling with `share` (0 to 1) of what is here; redrawn only when it changes (no animation).
  const { share, size = 18 }: { share: number; size?: number } = $props();
  // The ring's length at r = 8.
  const RING = 2 * Math.PI * 8;
</script>

<svg class="ring" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2" opacity="0.3" />
  <circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2" stroke-dasharray="{share * RING} {RING}" transform="rotate(-90 12 12)" />
</svg>

<style>
  .ring {
    flex: none;
  }
</style>
