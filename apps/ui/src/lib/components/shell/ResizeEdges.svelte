<script lang="ts">
  import { getApp } from '../../context';
  import { needsResizeEdges, startResize } from '../../windowChrome';

  const { settings } = getApp();
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

{#if settings.current.ownDecorations && needsResizeEdges}
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
