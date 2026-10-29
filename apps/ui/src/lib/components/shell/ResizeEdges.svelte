<script lang="ts">
  import { getApp } from '../../context';
  import { needsResizeEdges, startResize, watchMaximised } from '../../windowChrome';

  const { settings } = getApp();
  const shown = $derived(settings.current.ownDecorations && needsResizeEdges);
  // A maximised window can't be resized, so its edges shouldn't offer to.
  let maximised = $state(false);
  $effect(() => {
    if (shown) return watchMaximised((m) => (maximised = m));
  });
  // Edge names are the Tauri ResizeDirection values; the cursor is the CSS one.
  const edges = [
    ['North', 'n-resize', 'top: 0; left: 8px; right: 8px; height: 4px'],
    ['South', 's-resize', 'bottom: 0; left: 8px; right: 8px; height: 4px'],
    ['West', 'w-resize', 'left: 0; top: 8px; bottom: 8px; width: 4px'],
    ['East', 'e-resize', 'right: 0; top: 8px; bottom: 8px; width: 4px'],
    ['NorthWest', 'nw-resize', 'top: 0; left: 0; width: 8px; height: 8px'],
    ['NorthEast', 'ne-resize', 'top: 0; right: 0; width: 8px; height: 8px'],
    ['SouthWest', 'sw-resize', 'bottom: 0; left: 0; width: 8px; height: 8px'],
    ['SouthEast', 'se-resize', 'bottom: 0; right: 0; width: 8px; height: 8px'],
  ];
</script>

{#if shown && !maximised}
  {#each edges as [direction, cursor, place] (direction)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="edge" style="{place}; cursor: {cursor}" onpointerdown={(e) => e.button === 0 && startResize(direction)}></div>
  {/each}
{/if}

<style>
  .edge {
    position: fixed;
    z-index: 1000;
  }
</style>
