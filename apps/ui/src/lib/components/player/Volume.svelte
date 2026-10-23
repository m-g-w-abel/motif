<script lang="ts">
  import { getApp } from '../../context';
  import Icon from '../Icon.svelte';
  import Slider from '../Slider.svelte';

  const { engine } = getApp();
  const volume = $derived(engine.player?.volume ?? 1);

  // A drag fires many moves per frame; send at most one command per frame,
  // always ending on the latest value.
  let pending: number | null = null;
  function setVolume(v: number) {
    if (pending === null) {
      requestAnimationFrame(() => {
        engine.run({ type: 'set_volume', volume: pending ?? v });
        pending = null;
      });
    }
    pending = Math.round(v * 100) / 100;
  }
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
      oninput={setVolume}
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
