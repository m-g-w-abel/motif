<script lang="ts">
  import { getApp } from '../../context';
  import { trackTitle } from '../../format';

  const { engine } = getApp();
  const track = $derived(engine.current?.track);
</script>

<div class="label">
  <span class="title">{track ? trackTitle(track) : 'Nothing playing'}</span>
  <span class="sub">
    <!-- Apart, so each travels to its own place in now playing (`ExpandedPlayer`). -->
    {#if track}<span class="artist">{track.artist ?? 'Unknown artist'}</span>{#if track.album}<span class="sep">{' · '}</span><span class="album"
          >{track.album}</span
        >{/if}{:else}—{/if}
  </span>
</div>

<style>
  .label {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* The sheet's title weight, so the morph's ghost matches it (#109). */
  .title {
    font-weight: 700;
  }

  .sub {
    font-size: 0.92em;
    color: var(--text-muted);
  }

  /* Now playing's weights, so the morph's moving words keep their shape (#180). */
  .artist {
    font-weight: 600;
  }

  .album {
    font-weight: 700;
  }
</style>
