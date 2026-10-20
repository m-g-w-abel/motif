<script lang="ts">
  import { getApp } from '../../context';
  import { formatTime } from '../../format';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';
  import RepeatButton from './RepeatButton.svelte';
  import SeekBar from './SeekBar.svelte';
  import TrackLabel from './TrackLabel.svelte';
  import Transport from './Transport.svelte';
  import Volume from './Volume.svelte';

  const { engine, position, sheet } = getApp();
  const duration = $derived(engine.current?.track.duration_ms ?? 0);

  /**
   * Dragging up anywhere on the bar pulls the now-playing sheet out of it;
   * tapping the track expands it. Controls keep their own clicks.
   */
  function onPointerDown(e: PointerEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button:not(.identity), [role="slider"]')) return;
    const onTrack = !!target.closest('.identity');
    sheet.drag(e, 'bar', onTrack ? () => sheet.expand() : undefined);
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="bar" aria-label="Player" onpointerdown={onPointerDown}>
  <!-- The seek bar runs along the whole top edge, like a progress line. -->
  <div class="strip"><SeekBar variant="strip" /></div>

  <div class="row">
    <div class="transport"><Transport size={17} /></div>

    <button class="identity" title="Expand player">
      <Artwork track={engine.current?.track} size={40} />
      <TrackLabel />
    </button>

    <div class="end">
      <span class="time">
        {formatTime(position.ms)}<span class="total">{' / '}{duration ? formatTime(duration) : '-:--'}</span>
      </span>
      <RepeatButton />
      <Volume />
      <button class="icon-button" aria-label="Expand player" title="Expand player" onclick={() => sheet.expand()}>
        <Icon name="chevron-up" size={18} />
      </button>
    </div>

    <div class="mobile-controls"><Transport compact size={20} /></div>
  </div>
</section>

<style>
  .bar {
    position: relative;
    flex: none;
    background: var(--surface);
    border-top: 1px solid var(--divider);
    touch-action: none;
  }

  .strip {
    position: absolute;
    left: 0;
    right: 0;
    top: -1px;
    z-index: 1;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    height: var(--bar-height);
    padding: 6px var(--space-3) 0;
  }

  .transport {
    flex: none;
  }

  .identity {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .end {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .time {
    font-family: var(--font-mono);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--accent);
  }

  .total {
    color: var(--text-muted);
  }

  .mobile-controls {
    display: none;
  }

  @media (max-width: 700px) {
    .row {
      gap: var(--space-2);
      height: 60px;
    }

    .transport,
    .end {
      display: none;
    }

    .mobile-controls {
      display: block;
    }
  }
</style>
