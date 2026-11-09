<script lang="ts">
  import { getApp } from '../../context';
  import Icon from '../Icon.svelte';

  let { size = 16 }: { size?: number } = $props();

  const { engine } = getApp();
  const on = $derived(engine.player?.shuffle ?? false);
</script>

{#if engine.can('playback')}
<button
  class="icon-button"
  class:active={on}
  aria-label="Shuffle"
  aria-pressed={on}
  title={on ? 'Shuffle on' : 'Shuffle off'}
  onclick={() => engine.run({ type: 'set_shuffle', on: !on })}
>
  <Icon name="shuffle" {size} />
</button>
{/if}
