<script lang="ts">
  import { getApp, getViewState } from '../../context';
  import Artwork from '../player/Artwork.svelte';

  /** In the main window a click expands the player; inside it, it doesn't. `playing` shows the playing track, not the selected one. */
  let { expands = true, playing = false }: { expands?: boolean; playing?: boolean } = $props();

  const { engine, sheet } = getApp();
  const view = getViewState();
  const track = $derived(playing ? engine.current?.track : (view.browse.selectedTrack ?? undefined));

  // The largest square that fits, measured rather than left to CSS so it
  // behaves the same in every webview.
  let width = $state(0);
  let height = $state(0);
  const PADDING = 12;
  const side = $derived(Math.max(0, Math.min(width, height) - 2 * PADDING));
</script>

<section class="artwork" bind:clientWidth={width} bind:clientHeight={height}>
  {#if side > 0}
    {#if expands}
      <button class="frame" title="Expand player" style:width="{side}px" onclick={() => sheet.expand()}>
        <Artwork {track} size={side} />
      </button>
    {:else}
      <div class="frame" style:width="{side}px"><Artwork {track} size={side} /></div>
    {/if}
  {/if}
</section>

<style>
  .artwork {
    display: grid;
    place-items: center;
    height: 100%;
    min-height: 120px;
    background: var(--bg);
    overflow: hidden;
  }

  .frame {
    padding: 0;
    border: 0;
    background: none;
    line-height: 0;
  }

  button.frame {
    cursor: pointer;
  }

  .frame :global(.art) {
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.18);
  }
</style>
