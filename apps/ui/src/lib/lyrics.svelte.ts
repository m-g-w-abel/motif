import type { EngineClient } from './engine.svelte';
import type { Lyrics } from './protocol/Lyrics';
import type { LyricsLine } from './protocol/LyricsLine';

/**
 * The synced line showing at `ms` into the track with the lines moved
 * `offsetMs` later: the last one started, or -1 before the first.
 */
export function lineAt(lines: LyricsLine[], ms: number, offsetMs: number): number {
  let lo = 0;
  let hi = lines.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((lines[mid].time_ms ?? 0) + offsetMs <= ms) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** When the line after `index` starts, with the offset, or `null` after the last. */
export function nextStart(lines: LyricsLine[], index: number, offsetMs: number): number | null {
  const next = lines[index + 1]?.time_ms;
  return next == null ? null : next + offsetMs;
}

const KEY = 'lyrics-offsets';

/** Per-track lyrics offsets in ms (positive is later), kept on this device until saved to a sidecar. */
class Offsets {
  #all = $state<Record<string, number>>({});

  constructor() {
    try {
      this.#all = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    } catch {
      // None.
    }
  }

  get(track: string): number {
    return this.#all[track] ?? 0;
  }

  set(track: string, ms: number) {
    if (ms) this.#all[track] = ms;
    else delete this.#all[track];
    try {
      localStorage.setItem(KEY, JSON.stringify(this.#all));
    } catch {
      // Applies until reload.
    }
  }
}

export const lyricsOffsets = new Offsets();

/** The last lookup, shared so a layout's widget and the sheet's ask once per track. */
let last: { path: string; lyrics: Promise<Lyrics | null> } | undefined;

/** The lyrics of the library track at `path`; `fresh` asks again (after a save). */
export function fetchLyrics(engine: EngineClient, path: string, fresh = false): Promise<Lyrics | null> {
  if (fresh || last?.path !== path) {
    const lyrics = engine.query({ type: 'lyrics', path }).then((r) => (r.type === 'lyrics' ? r.lyrics : null));
    // A failed lookup is asked again next time.
    lyrics.catch(() => last?.lyrics === lyrics && (last = undefined));
    last = { path, lyrics };
  }
  return last.lyrics;
}
