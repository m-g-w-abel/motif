<script lang="ts">
  import Icon from './Icon.svelte';

  /** A loved song's heart, drawn only for one (never an empty heart); pressing it unloves. Loving is in menus. */
  let { size = 20, unlove }: { size?: number; unlove: () => void } = $props();
</script>

<button class="icon-button heart" aria-label="Unlove" title="Unlove" onclick={unlove}>
  <Icon name="heart" {size} solid />
</button>

<style>
  .heart {
    color: var(--accent);
  }
</style>
