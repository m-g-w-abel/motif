<script lang="ts">
  import { getApp } from '../../context';
  import { formatTime, trackFormat, trackTitle } from '../../format';
  import type { InfoStyle } from '../../layout/model';

  let { style }: { style: InfoStyle } = $props();

  const { engine, library } = getApp();
  const track = $derived(engine.current?.track);
  const queue = $derived(engine.queue);
  const position = $derived(engine.player?.current ?? null);
  /** Format details from the library, when the file is in it. */
  const details = $derived(library.details(track));
  const format = $derived(details?.codec ?? (track ? trackFormat(track) : ''));

  const rows = $derived<[string, string][]>(
    track
      ? (
          [
            ['Title', trackTitle(track)],
            ['Artist', track.artist ?? ''],
            ['Album', track.album ?? ''],
            ['Queue', position !== null ? `${position + 1}/${queue.length}` : ''],
            ['Duration', track.duration_ms ? formatTime(track.duration_ms) : ''],
            ['Year', details?.year ? String(details.year) : ''],
            ['Genre', details?.genre ?? ''],
            ['Format', format],
            ['Sample rate', details?.sample_rate ? `${details.sample_rate / 1000} kHz` : ''],
            ['Bit depth', details?.bit_depth ? `${details.bit_depth}-bit` : ''],
            ['Bitrate', details?.bitrate ? `${details.bitrate} kbps` : ''],
            ['File', track.source === 'local' ? track.id : ''],
          ] as [string, string][]
        ).filter(([, value]) => value)
      : [],
  );
</script>

<section class="info selectable {style}">
  {#if style === 'hero'}
    <h1>{track ? trackTitle(track) : 'Nothing playing'}</h1>
    {#if track}
      <p class="hero-artist">{track.artist ?? 'Unknown artist'}</p>
      <p class="hero-album">
        {track.album ?? ''}
        {#if format}<span class="format">{format}</span>{/if}
      </p>
    {:else}
      <p class="hero-album">Double-click a track in the library to start.</p>
    {/if}
  {:else if !track}
    <p class="empty">Nothing playing</p>
  {:else if style === 'list'}
    <h2>{trackTitle(track)}</h2>
    <p class="artist">{track.artist ?? 'Unknown artist'}</p>
    <dl>
      {#if track.album}<dd>{track.album}</dd>{/if}
      {#if track.duration_ms}<dd>{formatTime(track.duration_ms)}</dd>{/if}
      {#if format}<dd>{format}</dd>{/if}
    </dl>
  {:else}
    <table>
      <tbody>
        {#each rows as [label, value] (label)}
          <tr>
            <th scope="row">{label}</th>
            <td title={value}>{value}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .info {
    height: 100%;
    overflow: auto;
    padding: var(--space-3) var(--space-4);
    background: var(--bg);
  }

  /* Hero: large text for the now-playing screen. */
  .hero {
    height: auto;
    overflow: visible;
    padding: 0;
    background: none;
  }

  h1 {
    margin: 0;
    font-size: 1.8em;
    font-weight: 700;
    line-height: 1.15;
    letter-spacing: -0.01em;
  }

  .hero-artist {
    margin: var(--space-1) 0 0;
    font-size: 1.15em;
    font-weight: 600;
    color: var(--accent);
  }

  .hero-album {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 2px 0 0;
    color: var(--text-muted);
  }

  .format {
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .empty {
    margin: 0;
    color: var(--text-muted);
  }

  h2 {
    margin: 0;
    font-size: 1.05em;
    font-weight: 700;
  }

  .artist {
    margin: 2px 0 var(--space-4);
    color: var(--text-muted);
  }

  dl,
  dd {
    margin: 0;
  }

  dd {
    line-height: 1.6;
  }

  .table {
    padding: var(--space-2) 0;
  }

  table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
  }

  tr:nth-child(odd) {
    background: color-mix(in srgb, var(--surface) 55%, transparent);
  }

  th,
  td {
    height: var(--row-height);
    padding: 0 var(--space-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  th {
    width: 38%;
    font-size: 0.88em;
    font-weight: 600;
    color: var(--text-muted);
  }

  td {
    font-weight: 600;
  }
</style>
