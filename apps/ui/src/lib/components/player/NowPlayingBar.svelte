<script lang="ts">
  import { getApp } from '../../context';
  import { formatTime } from '../../format';
  import { gesture, mapped, slide } from '../../gestures';
  import { BAR_ART_SIZE } from '../../library.svelte';
  import Icon from '../Icon.svelte';
  import PhoneMenu from '../shell/PhoneMenu.svelte';
  import Artwork from './Artwork.svelte';
  import RepeatButton from './RepeatButton.svelte';
  import ShuffleButton from './ShuffleButton.svelte';
  import SeekBar from './SeekBar.svelte';
  import TrackLabel from './TrackLabel.svelte';
  import Transport from './Transport.svelte';
  import Volume from './Volume.svelte';
  import { dim, rise } from '../../screens';

  const { engine, position, sheet, settings, settingsView, views } = getApp();
  let sliding = $state<HTMLElement>();
  const duration = $derived(engine.current?.track.duration_ms ?? 0);

  /**
   * Dragging up anywhere on the bar pulls the now-playing sheet out of it;
   * tapping the track expands it, and a sideways swipe (touch) skips.
   * Controls keep their own clicks.
   */
  function onPointerDown(e: PointerEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button:not(.identity), [role="slider"], .grown')) return;
    views.menu = null;
    const onTrack = !!target.closest('.identity');
    gesture(
      e,
      mapped(settings.current.gestures, 'bar'),
      { next: () => engine.run({ type: 'next' }), previous: () => engine.run({ type: 'previous' }) },
      sliding ? slide(sliding) : undefined,
    );
    sheet.drag(e, 'bar', onTrack ? () => sheet.expand() : undefined);
  }

  /** The phone's Library and Search buttons: back to that root view, out of Settings. */
  // Only phones have the menu; a window widened with it open leaves it closed.
  const menu = $derived(views.phone && !!views.menu);

  function go(kind: 'home' | 'search') {
    views.show({ kind });
    settingsView.close();
  }
</script>

{#if menu}<button class="scrim" aria-label="Close menu" tabindex="-1" onclick={() => (views.menu = null)} transition:dim></button>{/if}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section bind:this={sheet.origin} class="bar" class:open={menu} aria-label="Player" onpointerdown={onPointerDown}>
  <!-- Over the music area, not pushing it up. -->
  <!-- It rises out of the bar's top edge, which clips it. -->
  {#if menu}<div class="grown"><div class="panel" transition:rise><PhoneMenu /></div></div>{/if}

  <div class="row">
    <div class="transport"><Transport size={17} /></div>

    <button class="identity" title="Expand player">
      <!-- Follows a touch swipe; the button clips it. -->
      <span class="slide" bind:this={sliding}>
        <Artwork track={engine.current?.track} size={40} exact={BAR_ART_SIZE} />
        <TrackLabel />
      </span>
    </button>

    <div class="end">
      <span class="time">
        {formatTime(position.ms)}<span class="total">{' / '}{duration ? formatTime(duration) : '-:--'}</span>
      </span>
      <ShuffleButton />
      <RepeatButton />
      <Volume />
      <button class="icon-button" aria-label="Expand player" title="Expand player" onclick={() => sheet.expand()}>
        <Icon name="chevron-up" size={18} />
      </button>
    </div>

    <div class="mobile-controls"><Transport compact size={20} /></div>
  </div>

  <!-- The seek bar runs along the whole top edge, like a progress line; on phones, under the track. -->
  <div class="strip"><SeekBar variant="strip" /></div>

  {#if views.phone}
    <nav class="buttons" aria-label="Views">
      <button class="icon-button" class:active={views.root === 'library' && !settingsView.group} aria-label="Library" onclick={() => go('home')}>
        <Icon name="grid" size={24} />
      </button>
      <button class="icon-button" class:active={views.root === 'search' && !settingsView.group} aria-label="Search" onclick={() => go('search')}>
        <Icon name="search" size={24} />
      </button>
      <button
        class="icon-button"
        class:active={menu}
        aria-label="Menu"
        aria-expanded={menu}
        onclick={() => (views.menu = menu ? null : 'main')}
      >
        <Icon name="menu" size={24} />
      </button>
    </nav>
  {/if}
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
    min-width: 0;
    overflow: hidden;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .slide {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
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

  .scrim {
    position: fixed;
    inset: 0;
    z-index: 29;
    padding: 0;
    border: 0;
    background: rgb(0 0 0 / 0.55);
  }

  .grown {
    position: absolute;
    left: -1px;
    right: -1px;
    bottom: 100%;
    overflow: hidden;
  }

  .panel {
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: 12px 12px 0 0;
    background: var(--surface);
  }

  .buttons {
    display: flex;
    justify-content: space-around;
    padding: var(--space-1) 0 var(--space-2);
  }

  .buttons .icon-button {
    width: 56px;
    height: 40px;
    color: var(--text-muted);
  }

  .buttons .icon-button.active {
    color: var(--text);
  }

  /* Phones: a card at the bottom, with the menu growing up out of it. */
  @media (max-width: 700px) {
    .bar {
      margin: 0 var(--space-2) var(--space-2);
      border: 1px solid var(--border);
      border-radius: 12px;
    }

    .bar.open {
      z-index: 30;
      border-top-color: transparent;
      border-radius: 0 0 12px 12px;
    }

    .row {
      gap: var(--space-2);
      height: 60px;
      padding-top: 0;
    }

    .strip {
      position: static;
      margin: 0 var(--space-3);
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
