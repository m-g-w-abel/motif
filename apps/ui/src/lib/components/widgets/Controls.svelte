<script lang="ts">
  import RepeatButton from '../player/RepeatButton.svelte';
  import SeekBar from '../player/SeekBar.svelte';
  import ShuffleButton from '../player/ShuffleButton.svelte';
  import Transport from '../player/Transport.svelte';
  import Volume from '../player/Volume.svelte';
</script>

<!-- Full playback controls: seek with times, then transport centred between shuffle and repeat, and volume. -->
<section class="controls">
  <SeekBar />
  <div class="row">
    <div class="side"><ShuffleButton size={18} /><RepeatButton size={18} /></div>
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

  /* Phones have hardware volume keys: shuffle and repeat go either side of the transport. */
  @media (max-width: 700px) {
    .row {
      display: flex;
      justify-content: space-between;
    }

    .side {
      display: contents;
    }

    .row > :global(.transport) {
      order: 1;
    }

    .row :global([aria-label^='Repeat']) {
      order: 2;
    }

    .end {
      display: none;
    }
  }
</style>
