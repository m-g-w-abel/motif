import { groupConsecutive } from './albums';
import { demoLibrary, isDemo } from './demo';
import type { EngineClient } from './engine.svelte';
import type { LibraryClient } from './library.svelte';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { SortRule } from './protocol/SortRule';
import type { TrackGroup } from './protocol/TrackGroup';
import type { TrackRef } from './protocol/TrackRef';

/** A list of tracks the widgets can show: the browsed library, or the play queue. */
export type ListSource = 'library' | 'queue';

/**
 * A list of tracks from one source, with what playing or removing means for
 * it. Rows are asked for one at a time because a library list is paged: a
 * row can be `undefined` while it loads.
 */
export interface TrackListData {
  title: string;
  kind: ListSource;
  readonly count: number;
  track(index: number): TrackRef | undefined;
  /** Library fields of a row, where the list has them. */
  details?(index: number): LibraryTrack | undefined;
  /** Stable per-row key. */
  key(index: number): string;
  /** Changes when the rows are replaced by another order or search, not when they refetch. */
  readonly order?: object;
  /** Album runs in list order. */
  readonly groups: TrackGroup[];
  readonly totalMs: number;
  /** Key of the row that is playing, if it is in this list. Getters, so lists stay put when only playback changes. */
  readonly currentKey: string | null;
  play(index: number): void;
  playGroup(group: TrackGroup): void;
  remove?(index: number): void;
  /** Why a row can't be played, where the engine has tried and failed. */
  error?(index: number): string | null;
  /** What to say when there are no rows. */
  readonly empty: { message: string; hint?: string; addFolder?: boolean };
  /** Keeps paged data fresh; call from an `$effect`. */
  sync?(): void;
  /** Pins the first visible row; `onShift` hears if rows inserted above move it. */
  anchor?(index: number, onShift: (index: number) => void): void;
}

export function trackListData(
  engine: EngineClient,
  library: LibraryClient,
  kind: ListSource,
  search: string | null = null,
  sort: SortRule[] = [],
): TrackListData {
  if (kind === 'queue') return queueData(engine);
  if (isDemo) return demoData(engine);
  return libraryData(engine, library, search, sort);
}

/** Memoises a value computed from the data object's inputs. */
function lazy<T>(make: () => T): () => T {
  let value: { v: T } | undefined;
  return () => (value ??= { v: make() }).v;
}

function queueData(engine: EngineClient): TrackListData {
  const queue = engine.queue;
  const groups = lazy(() => groupConsecutive(queue.map((item) => item.track)));
  return {
    title: 'Queue',
    kind: 'queue',
    count: queue.length,
    track: (i) => queue[i]?.track,
    key: (i) => String(queue[i]?.id ?? `#${i}`),
    get groups() {
      return groups();
    },
    get totalMs() {
      return groups().reduce((n, g) => n + g.duration_ms, 0);
    },
    get currentKey() {
      const current = engine.player?.current;
      return current != null && queue[current] ? String(queue[current].id) : null;
    },
    play: (i) => engine.run({ type: 'play_item', item_id: queue[i].id }),
    playGroup: (g) => engine.run({ type: 'play_item', item_id: queue[g.start].id }),
    remove: (i) => engine.run({ type: 'remove_item', item_id: queue[i].id }),
    error: (i) => queue[i]?.error ?? null,
    empty: { message: 'The queue is empty.', hint: 'Double-click a track in the library to play it.' },
  };
}

/** Development only: the generated demo library, played as a simulation. */
function demoData(engine: EngineClient): TrackListData {
  const groups = lazy(() => groupConsecutive(demoLibrary));
  return {
    title: 'Library',
    kind: 'library',
    count: demoLibrary.length,
    track: (i) => demoLibrary[i],
    key: (i) => demoLibrary[i]?.id ?? `#${i}`,
    get groups() {
      return groups();
    },
    get totalMs() {
      return groups().reduce((n, g) => n + g.duration_ms, 0);
    },
    get currentKey() {
      return engine.current?.track.id ?? null;
    },
    play: (start) => engine.run({ type: 'set_queue', tracks: demoLibrary, start }),
    playGroup: (g) =>
      engine.run({ type: 'set_queue', tracks: demoLibrary.slice(g.start, g.start + g.count), start: 0 }),
    empty: { message: 'No demo tracks.' },
  };
}

function libraryData(
  engine: EngineClient,
  library: LibraryClient,
  search: string | null,
  sort: SortRule[],
): TrackListData {
  const list = library.tracks(search, sort);
  const run = (query: Parameters<EngineClient['query']>[0]) =>
    engine.query(query).catch((e: Error) => (engine.error = e.message));
  return {
    title: 'Library',
    kind: 'library',
    order: list,
    get count() {
      return list.total;
    },
    track: (i) => list.get(i)?.track,
    details: (i) => list.get(i),
    key: (i) => list.get(i)?.track.id ?? `#${i}`,
    get groups() {
      return list.groups;
    },
    get totalMs() {
      return list.groups.reduce((n, g) => n + g.duration_ms, 0);
    },
    get currentKey() {
      const track = engine.current?.track;
      return track?.source === 'local' ? track.id : null;
    },
    play: (start) => void run({ type: 'play', search, sort, start }),
    playGroup: (g) => void run({ type: 'play_album', album: g.key, sort }),
    // The library keeps tracks whose files are gone or damaged, so they can
    // be found or replaced later; they show why they won't play.
    error: (i) => {
      const t = list.get(i);
      return t?.missing ? 'File missing' : (t?.problem ?? null);
    },
    get empty() {
      const status = library.status;
      if (!status) return { message: 'This engine has no library.' };
      if (status.roots.length === 0) {
        return { message: 'No music folders yet.', hint: 'Add a folder and Motif will find the music in it.', addFolder: true };
      }
      if (search) return { message: `Nothing matches “${search}”.` };
      if (status.scanning) return { message: 'Scanning your music…', hint: `${status.scanned} files so far` };
      return { message: 'No music found in your folders.', addFolder: true };
    },
    sync: () => list.sync(),
    anchor: (index, onShift) => {
      list.onShift = onShift;
      list.setAnchor(index);
    },
  };
}

