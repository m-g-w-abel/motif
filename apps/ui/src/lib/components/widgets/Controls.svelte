<script lang="ts">
  import RepeatButton from '../player/RepeatButton.svelte';
  import SeekBar from '../player/SeekBar.svelte';
  import Transport from '../player/Transport.svelte';
  import Volume from '../player/Volume.svelte';
</script>

<!-- Full playback controls: seek with times, then transport centred between repeat and volume. -->
<section class="controls">
  <SeekBar />
  <div class="row">
    <div class="side"><RepeatButton size={18} /></div>
    <Transport size={26} />
    <div class="side end"><Volume /></div>
  </div>
</section>

<style>
  .controls {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: 100%;
  }

  .row {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
  }

  .side {
    display: flex;
    min-width: 0;
  }

  .end {
    justify-content: flex-end;
  }

  /* Phones have hardware volume keys; keep the transport uncrowded. */
  @media (max-width: 700px) {
    .end {
      visibility: hidden;
    }
  }
</style>
