<script lang="ts">
  import { getApp } from '../../context';
  import Icon from '../Icon.svelte';
  import Slider from '../Slider.svelte';

  const { engine } = getApp();
  const volume = $derived(engine.player?.volume ?? 1);
</script>

<div class="volume" title="Volume {Math.round(volume * 100)}%">
  <Icon name="volume" size={16} />
  <div class="slider">
    <Slider
      value={volume}
      max={1}
      label="Volume"
      valueText="{Math.round(volume * 100)}%"
      variant="muted"
      oninput={(v) => engine.run({ type: 'set_volume', volume: Math.round(v * 100) / 100 })}
    />
  </div>
</div>

<style>
  .volume {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--text-muted);
  }

  .slider {
    width: 90px;
  }
</style>
