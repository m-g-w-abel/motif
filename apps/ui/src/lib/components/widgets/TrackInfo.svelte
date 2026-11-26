<script lang="ts">
  import type { Snippet } from 'svelte';
  import { getApp, getViewState } from '../../context';
  import { formatTime, playedFormat, trackFormat, trackTitle } from '../../format';
  import { fade } from 'svelte/transition';
  import { elsewhere } from '../../devices';
  import { marquee } from '../../marquee';
  import { DURATION } from '../../motion/transition';
  import type { InfoStyle } from '../../layout/model';
  import Heart from '../Heart.svelte';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import Stars from '../Stars.svelte';

  /** `playing` shows the playing track (now-playing screen); otherwise the track selected in a list. */
  let { style, playing = false, aside }: { style: InfoStyle; playing?: boolean; /** Hero: beside the title and artist. */ aside?: Snippet } = $props();

  const { engine, library, settings, sheet } = getApp();
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

  /** Hero names that don't fit one line (phones) scroll once per track while the sheet is open. */
  // Held where it is while the sheet moves (its change starts from the text as drawn), put back once it is closed.
  const scrolling = $derived(style === 'hero' && playing && track ? (sheet.rest ? track.id : sheet.moving ? false : null) : null);
  /** What arrives (Opus if it is transcoded), and a stream's device; a song from elsewhere fades in once that is known. */
  const shown = $derived(
    playedFormat(
      details,
      details?.codec ?? (track ? trackFormat(track) : ''),
      playing ? engine.player?.streamed : null,
      playing && !!details && elsewhere(details),
    ),
  );
  const format = $derived(shown.format);
  /** A song from elsewhere whose stream isn't known yet: its badge keeps its place, unseen, so nothing moves when it shows. */
  const pending = $derived(playing && !!details && elsewhere(details) && !engine.player?.streamed);
  const transcoded = $derived(playing && !!engine.player?.streamed && engine.player.streamed.sent !== 'original');
  /** Before the format on phones: "48 kHz · 152 kbps · ". */
  const rates = $derived(
    [shown.rate, shown.bitrate]
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
            ['Sample rate', shown.rate],
            ['Bit depth', details?.bit_depth && !transcoded ? `${details.bit_depth}-bit` : ''],
            ['Bitrate', shown.bitrate],
            ['ReplayGain', playing && gain != null ? `${gain > 0 ? '+' : ''}${gain.toFixed(2)} dB` : ''],
            ['File', track.source === 'local' ? track.id : ''],
            ['Device', shown.device],
          ] as [string, string][]
        ).filter(([, value]) => value)
      : [],
  );
</script>

<section class="info selectable {style}" bind:this={scroller}>
  {#if style === 'hero'}
    <div class="hero-title">
      <div class="names">
        <h1 use:marquee={scrolling}><span class="marquee" data-move={playing ? 'title:now' : undefined}>{track ? trackTitle(track) : none}</span></h1>
        {#if track}<p class="hero-artist" use:marquee={scrolling}><span class="marquee" data-move={playing ? 'artist:now' : undefined}>{track.artist ?? 'Unknown artist'}</span></p>{/if}
      </div>
      {@render aside?.()}
    </div>
    {#if track}
      <p class="hero-album" data-enter="fade">
        <span class="album-name" data-move={playing ? 'album:now' : undefined}>{track.album ?? ''}</span>
        {#if format || pending}<span class="format" class:pending><span class="rates">{rates}</span>{format || '\u00a0'}</span>{/if}
      </p>
      {#if shown.device}<p class="device" data-enter="fade" in:fade={{ duration: DURATION }}><Icon name="cloud" size={12} /> {shown.device}</p>{/if}
      {#if playing && details && engine.can('library')}
        <div class="marks" data-enter="fade">
          {#if settings.current.ratings === 'stars'}
            <Stars rating={details.rating} size={26} set={(rating) => library.setRating([track], rating)} />
          {:else if details.favourite}
            <Heart size={22} unlove={() => library.setFavourite([track], false)} />
          {/if}
        </div>
      {/if}
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
    position: relative;
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

  /* One box, wrapped across the names' width: an inline one spans only its longest line, and the sheet's moving copy (a box that size) would wrap differently. */
  .names .marquee {
    display: inline-block;
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

  /* Where a stream comes from, just under the file info in the gap the now-playing screen keeps below it, so it moves nothing; a long name ends in an ellipsis. */
  .device {
    position: absolute;
    top: 100%;
    right: 0;
    left: 0;
    margin: var(--space-1) 0 0;
    overflow: hidden;
    font-size: 0.85em;
    color: var(--text-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .device :global(svg) {
    vertical-align: -1px;
  }

  .format.pending {
    opacity: 0;
  }

  .marks {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-2);
  }

  .format {
    transition: opacity 260ms;
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
