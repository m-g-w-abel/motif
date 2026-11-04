<script lang="ts">
  import { getApp } from '../../context';
  import type { RepeatMode } from '../../protocol/RepeatMode';
  import Icon from '../Icon.svelte';

  let { size = 16 }: { size?: number } = $props();

  const { engine } = getApp();
  const repeat = $derived(engine.player?.repeat ?? 'off');
  const next: Record<RepeatMode, RepeatMode> = { off: 'all', all: 'one', one: 'off' };
  const labels: Record<RepeatMode, string> = { off: 'Repeat off', all: 'Repeat all', one: 'Repeat one' };
</script>

{#if engine.can('playback')}
<button
  class="icon-button"
  class:active={repeat !== 'off'}
  aria-label={labels[repeat]}
  title={labels[repeat]}
  onclick={() => engine.run({ type: 'set_repeat', mode: next[repeat] })}
>
  <Icon name="repeat" {size} />
  {#if repeat === 'one'}<span class="badge">1</span>{/if}
</button>
{/if}

<style>
  .icon-button {
    position: relative;
  }

  .badge {
    position: absolute;
    right: 3px;
    bottom: 2px;
    font-size: 9px;
    font-weight: 700;
  }
</style>
