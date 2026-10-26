<script lang="ts">
  import type { LayoutNode, Size } from '../../layout/model';
  // Every widget a layout can place. Layouts only choose and configure these.
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import Artwork from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';
  import TrackList from '../widgets/TrackList.svelte';
  import Self from './LayoutNode.svelte';

  let { node }: { node: LayoutNode } = $props();

  function flex(size: Size | undefined): string {
    if (!size) return '1 1 0';
    return 'px' in size ? `0 0 ${size.px}px` : `0 0 ${size.percent}%`;
  }
</script>

{#if node.kind === 'split'}
  <div class="split {node.direction}">
    {#each node.children as child, i (i)}
      <div class="slot" class:sized={child.size} style:flex={flex(child.size)}>
        <Self node={child} />
      </div>
    {/each}
  </div>
{:else if node.panel === 'tracklist'}
  <TrackList style={node.style} columns={node.columns} maxWidth={node.maxWidth} />
{:else if node.panel === 'album-grid'}
  <AlbumGrid cardSize={node.cardSize} columns={node.columns} />
{:else if node.panel === 'artwork'}
  <Artwork />
{:else if node.panel === 'track-info'}
  <TrackInfo style={node.style} />
{:else if node.panel === 'controls'}
  <div class="controls"><Controls /></div>
{:else if node.panel === 'queue'}
  <Queue collapsible={node.collapsible} />
{/if}

<style>
  /* Panels are separated by 1px rules: the split's background shows through the gap. */
  .split {
    display: flex;
    gap: 1px;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--border);
  }

  .split.row {
    flex-direction: row;
  }

  .split.column {
    flex-direction: column;
  }

  .controls {
    display: grid;
    place-items: center;
    height: 100%;
    padding: var(--space-3);
  }

  .slot {
    position: relative;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg);
  }

  /*
   * Phones stack row splits. Fixed-size slots (side lists) shrink to their
   * content; flexible ones get most of a screen.
   */
  @media (max-width: 700px) {
    .split.row {
      flex-direction: column;
      height: auto;
      min-height: 100%;
    }

    .split.row > .slot {
      flex: none !important;
      min-height: 70vh;
    }

    .split.row > .slot.sized {
      min-height: 0;
    }
  }
</style>
