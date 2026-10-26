<script lang="ts">
  import { getApp, setViewState } from '../../context';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ArtworkWidget from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';

  const { layout, sheet } = getApp();
  let body = $state<HTMLElement>();
  setViewState({ source: 'library', layoutId: 'now-playing', browse: layout.browseState('now-playing') });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && sheet.progress === 1) sheet.collapse();
  }
</script>

<svelte:window onkeydown={onKey} />

<!--
  Grows out of the player bar: translated by (1 - progress) of its height, so
  dragging the bar or the sheet moves it with the finger. Built from the same
  widgets layouts use.
-->
<section
  class="sheet"
  class:dragging={sheet.dragging}
  aria-label="Now playing"
  aria-hidden={!sheet.open}
  inert={!sheet.open}
  style:transform="translateY({(1 - sheet.progress) * 100}%)"
>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header onpointerdown={(e) => sheet.drag(e, 'sheet')}>
    <span class="handle"></span>
    <button class="icon-button" aria-label="Collapse" title="Collapse (Esc)" onclick={() => sheet.collapse()}>
      <Icon name="chevron-down" size={20} />
    </button>
  </header>

  <div class="body" bind:this={body}>
    <div class="now">
      <div class="art"><ArtworkWidget expands={false} /></div>
      <div class="details">
        <TrackInfo style="hero" playing />
        <Controls />
      </div>
    </div>
    <div class="queue-slot">
      <Queue collapsible />
    </div>
  </div>
  <Scrollbar target={body} />
</section>

<style>
  .sheet {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    transition: transform 260ms cubic-bezier(0.2, 0.8, 0.2, 1);
    will-change: transform;
  }

  .sheet.dragging {
    transition: none;
  }

  header {
    position: relative;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    height: 40px;
    padding: 0 var(--space-2);
    flex: none;
    touch-action: none;
    cursor: grab;
  }

  .handle {
    position: absolute;
    left: 50%;
    top: 8px;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--border);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* Artwork beside the details; together they take whatever the queue leaves. */
  .now {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(300px, 460px) minmax(0, 0.4fr);
    align-items: center;
    gap: var(--space-6);
    padding: 0 var(--space-6) var(--space-6);
  }

  .art {
    height: 100%;
    min-height: 0;
  }

  .details {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  /* The queue sits underneath: a header when folded, ~40% of the height when open. */
  .queue-slot {
    flex: none;
    border-top: 1px solid var(--divider);
  }

  .queue-slot:has(:global(.queue.open)) {
    flex: 0 1 40%;
    min-height: 160px;
  }

  /* Phones: cover the whole screen; artwork, details and queue stack and scroll. */
  @media (max-width: 700px) {
    .sheet {
      position: fixed;
      z-index: 45;
      padding-top: env(safe-area-inset-top);
      padding-bottom: env(safe-area-inset-bottom);
    }

    .body {
      /* Holds the bars of the scroll areas inside it, so they scroll with it. */
      position: relative;
      overflow-y: auto;
    }

    .now {
      flex: none;
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-4);
      padding: 0 var(--space-4) var(--space-4);
    }

    .art {
      height: min(55vh, calc(100vw - 2 * var(--space-4)));
    }

    .queue-slot:has(:global(.queue.open)) {
      flex: none;
      height: 60vh;
    }
  }
</style>
