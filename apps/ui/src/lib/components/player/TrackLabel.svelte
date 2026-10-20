<script lang="ts">
  import { getApp } from '../../context';
  import { trackTitle } from '../../format';

  const { engine } = getApp();
  const track = $derived(engine.current?.track);
</script>

<div class="label">
  <span class="title">{track ? trackTitle(track) : 'Nothing playing'}</span>
  <span class="sub">
    {#if track}{track.artist ?? 'Unknown artist'}{#if track.album}{' · '}{track.album}{/if}{:else}—{/if}
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

  .title {
    font-weight: 600;
  }

  .sub {
    font-size: 0.92em;
    color: var(--text-muted);
  }
</style>
