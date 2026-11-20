<script lang="ts">
  import Icon from './Icon.svelte';

  let { on, size = 20, set }: { on: boolean; size?: number; set: (on: boolean) => void } = $props();
</script>

<button class="icon-button heart" class:on aria-label="Favourite" aria-pressed={on} title={on ? 'Remove from favourites' : 'Add to favourites'} onclick={() => set(!on)}>
  <Icon name="heart" {size} solid={on} />
</button>

<style>
  .heart.on {
    color: var(--accent);
  }
</style>
