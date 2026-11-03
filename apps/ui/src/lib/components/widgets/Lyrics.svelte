<script lang="ts" module>
  import type { LyricsSource } from '../../protocol/LyricsSource';

  const SOURCES: Record<LyricsSource, string> = { embedded: 'Embedded', sidecar: 'Sidecar', lrclib: 'LRCLIB' };
</script>

<script lang="ts">
  import { getApp } from '../../context';
  import { isDemo } from '../../demo';
  import { fetchLyrics, lineAt, lyricsOffsets, nextStart } from '../../lyrics.svelte';
  import type { Lyrics } from '../../protocol/Lyrics';
  import Scrollbar from '../Scrollbar.svelte';

  const { engine, position } = getApp();
  /** The playing library track, by path. */
  const path = $derived(engine.current?.track.source === 'local' && !isDemo ? engine.current.track.id : null);
  /** `undefined` while it is looked up. */
  let lyrics = $state<Lyrics | null | undefined>(null);
  const synced = $derived(lyrics?.lines[0]?.time_ms != null ? lyrics.lines : null);
  const offset = $derived(path ? lyricsOffsets.get(path) : 0);

  function load(fresh = false) {
    const asked = path;
    if (!asked) return void (lyrics = null);
    lyrics = undefined;
    fetchLyrics(engine, asked, fresh)
      .catch(() => null)
      .then((found) => {
        if (path === asked) lyrics = found;
      });
  }
  $effect(() => load());

  /** The line being sung, changed by a timer set for the next line's start: nothing runs between lines. */
  let current = $state(-1);
  $effect(() => {
    const lines = synced;
    const shift = offset;
    if (!lines) return;
    const playing = engine.player?.status === 'playing';
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = () => {
      const ms = position.at(Date.now());
      current = lineAt(lines, ms, shift);
      const next = nextStart(lines, current, shift);
      if (playing && next !== null) timer = setTimeout(tick, next - ms + 1);
    };
    tick();
    return () => clearTimeout(timer);
  });

  let rows = $state<HTMLElement[]>([]);
  let height = $state(0);
  let width = $state(0);
  /** Moves the list so the current line sits in the middle. */
  const scroll = $derived.by(() => {
    void width;
    const row = rows[Math.max(current, 0)];
    return row ? height / 2 - row.offsetTop - row.offsetHeight / 2 : 0;
  });
  let plainBox = $state<HTMLElement>();

  function nudge(by: number) {
    if (path) lyricsOffsets.set(path, Math.round((offset + by) / 100) * 100);
  }

  async function save() {
    if (!path) return;
    try {
      await engine.query({ type: 'save_lyrics', path, offset_ms: offset });
      lyricsOffsets.set(path, 0);
      load(true);
    } catch (e) {
      engine.error = (e as Error).message;
    }
  }

  const seconds = (ms: number) => `${ms > 0 ? '+' : ms < 0 ? '−' : ''}${Math.abs(ms / 1000).toFixed(1)} s`;
</script>

<section class="lyrics">
  {#if synced}
    <div class="view synced" bind:clientHeight={height} bind:clientWidth={width}>
      <ol style:transform="translateY({scroll}px)">
        {#each synced as line, i (i)}
          <li bind:this={rows[i]}>
            <button
              class:current={i === current}
              class:past={i < current}
              title="Play from here"
              onclick={() => engine.run({ type: 'seek', position_ms: Math.max(0, (line.time_ms ?? 0) + offset) })}
            >
              {line.text || '♪'}
            </button>
          </li>
        {/each}
      </ol>
    </div>
  {:else if lyrics}
    <div class="view plain custom-scrollbar selectable" bind:this={plainBox}>
      {#each lyrics.lines as line, i (i)}
        <p>{line.text || ' '}</p>
      {/each}
    </div>
    <Scrollbar target={plainBox} />
  {:else}
    <p class="empty">{lyrics === undefined ? '' : engine.current ? 'No lyrics' : 'Nothing playing'}</p>
  {/if}
  {#if lyrics}
    <footer>
      <span class="source">{SOURCES[lyrics.source]}</span>
      {#if synced}
        <span class="offset" role="group" aria-label="Lyrics offset">
          <button class="icon-button" aria-label="Show lyrics earlier" title="Earlier (−0.1 s)" onclick={() => nudge(-100)}>−</button>
          <button class="value" title="Offset; click to reset" disabled={!offset} onclick={() => path && lyricsOffsets.set(path, 0)}>{seconds(offset)}</button>
          <button class="icon-button" aria-label="Show lyrics later" title="Later (+0.1 s)" onclick={() => nudge(100)}>+</button>
        </span>
        <button class="save" title="Write these lyrics, offset included, to a .lrc file beside the track" disabled={lyrics.source === 'sidecar' && !offset} onclick={save}>
          Save to sidecar
        </button>
      {/if}
    </footer>
  {/if}
</section>

<style>
  .lyrics {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    background: var(--bg);
  }

  .view {
    flex: 1;
    min-height: 0;
  }

  .synced {
    overflow: hidden;
  }

  ol {
    margin: 0;
    padding: 0 var(--space-4);
    list-style: none;
    transition: transform 300ms cubic-bezier(0.2, 0.8, 0.2, 1);
  }

  li button {
    display: block;
    width: 100%;
    padding: var(--space-1) 0;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 1.15em;
    font-weight: 600;
    line-height: 1.35;
    text-align: left;
    cursor: pointer;
    opacity: 0.55;
    transition:
      color 150ms linear,
      opacity 150ms linear;
  }

  li button.past {
    opacity: 0.4;
  }

  li button.current {
    color: var(--text);
    opacity: 1;
  }

  li button:hover {
    opacity: 1;
  }

  .plain {
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
  }

  .plain p {
    margin: 0;
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .empty {
    margin: auto;
    color: var(--text-muted);
  }

  footer {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-3);
    border-top: 1px solid var(--divider);
    font-size: 11px;
    color: var(--text-muted);
  }

  .source {
    margin-right: auto;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .offset {
    display: inline-flex;
    align-items: center;
  }

  .offset .icon-button {
    width: 24px;
    height: 24px;
    font-size: 14px;
  }

  .value,
  .save {
    padding: 2px var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .value {
    min-width: 4.5em;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }

  .save {
    border: 1px solid var(--border);
    white-space: nowrap;
  }

  .value:disabled,
  .save:disabled {
    cursor: default;
    opacity: 0.5;
  }
</style>
