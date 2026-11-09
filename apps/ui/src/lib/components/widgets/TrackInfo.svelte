<script lang="ts">
  import type { Snippet } from 'svelte';
  import { getApp, getViewState } from '../../context';
  import { formatTime, trackFormat, trackTitle } from '../../format';
  import type { InfoStyle } from '../../layout/model';
  import Scrollbar from '../Scrollbar.svelte';

  /** `playing` shows the playing track (now-playing screen); otherwise the track selected in a list. */
  let { style, playing = false, aside }: { style: InfoStyle; playing?: boolean; /** Hero: beside the title and artist. */ aside?: Snippet } = $props();

  const { engine, library } = getApp();
  const view = getViewState();
  let scroller = $state<HTMLElement>();
  const track = $derived(playing ? engine.current?.track : (view.browse.selectedTrack ?? undefined));
  const none = $derived(playing ? 'Nothing playing' : 'Nothing selected');
  const context = $derived(engine.player?.context);
  /** Format details from the library, when the file is in it. */
  const details = $derived(library.details(track));
  /** The ReplayGain adjustment the engine applies to what is playing. */
  const gain = $derived(engine.player?.gain_db);
  /** The original year, with the release's own when it differs: "1977 (2011 release)". */
  function yearLabel(d: { year?: number | null; original_year?: number | null } | null | undefined): string {
    if (!d?.original_year) return d?.year ? String(d.year) : '';
    return d.year && d.year !== d.original_year ? `${d.original_year} (${d.year} release)` : String(d.original_year);
  }

  const format = $derived(details?.codec ?? (track ? trackFormat(track) : ''));
  /** Before the format on phones: "48 kHz · 152 kbps · ". */
  const rates = $derived(
    [details?.sample_rate && `${details.sample_rate / 1000} kHz`, details?.bitrate && `${details.bitrate} kbps`]
      .filter(Boolean)
      .map((v) => `${v} · `)
      .join(''),
  );

  const rows = $derived<[string, string][]>(
    track
      ? (
          [
            ['Title', trackTitle(track)],
            ['Artist', track.artist ?? ''],
            ['Album', track.album ?? ''],
            ['Queue', playing && context && context.index !== null ? `${context.index + 1}/${context.len}` : ''],
            ['Duration', track.duration_ms ? formatTime(track.duration_ms) : ''],
            ['Year', yearLabel(details)],
            ['Genre', details?.genre ?? ''],
            ['Format', format],
            ['Sample rate', details?.sample_rate ? `${details.sample_rate / 1000} kHz` : ''],
            ['Bit depth', details?.bit_depth ? `${details.bit_depth}-bit` : ''],
            ['Bitrate', details?.bitrate ? `${details.bitrate} kbps` : ''],
            ['ReplayGain', playing && gain != null ? `${gain > 0 ? '+' : ''}${gain.toFixed(2)} dB` : ''],
            ['File', track.source === 'local' ? track.id : ''],
          ] as [string, string][]
        ).filter(([, value]) => value)
      : [],
  );
</script>

<section class="info selectable {style}" bind:this={scroller}>
  {#if style === 'hero'}
    <div class="hero-title">
      <div class="names">
        <h1>{track ? trackTitle(track) : none}</h1>
        {#if track}<p class="hero-artist">{track.artist ?? 'Unknown artist'}</p>{/if}
      </div>
      {@render aside?.()}
    </div>
    {#if track}
      <p class="hero-album">
        <span class="album-name">{track.album ?? ''}</span>
        {#if format}<span class="format"><span class="rates">{rates}</span>{format}</span>{/if}
      </p>
    {:else}
      <p class="hero-album">Double-click a track in the library to start.</p>
    {/if}
  {:else if !track}
    <p class="empty">{none}{#if !playing}<br />Select a track in the list to see its details.{/if}</p>
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
<Scrollbar target={scroller} />

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

  .hero-title {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  /* Long names wrap here, even mid-word, rather than running under what sits beside them. */
  .names {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  /* The sample rate and bitrate are in the format badge on phones only. */
  .rates {
    display: none;
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
