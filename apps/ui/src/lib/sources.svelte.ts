import { groupConsecutive } from './albums';
import { demoLibrary, isDemo } from './demo';
import type { EngineClient } from './engine.svelte';
import { hold, type LibraryClient, type LibraryTracks, type RecentPlays, type Shown } from './library.svelte';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { SortRule } from './protocol/SortRule';
import type { TrackGroup } from './protocol/TrackGroup';
import type { TrackRef } from './protocol/TrackRef';

/** A list of tracks the widgets can show: the browsed library (saved playlists join it later). */
export type ListSource = 'library';

/**
 * A list of tracks from one source, with what playing means for it. Rows
 * are asked for one at a time because a library list is paged: a row can be
 * `undefined` while it loads.
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
  /** Loads rows `from` to `to` where they are fetched in pages, so `track` and `key` have them. */
  load?(from: number, to: number): Promise<void>;
  /** Pins the first visible row; `onShift` hears if rows inserted above move it. */
  anchor?(index: number, onShift: (index: number) => void): void;
}

/**
 * What a song view lists (`SongList.svelte`): the rows and what playing means, without album runs.
 * Its `currentKey` is the playing track's id, which a row with that track shows as playing.
 */
export type SongListData = Pick<
  TrackListData,
  'title' | 'count' | 'track' | 'details' | 'key' | 'order' | 'currentKey' | 'play' | 'error' | 'empty' | 'sync' | 'load'
>;

const localKey = (engine: EngineClient) => {
  const track = engine.current?.track;
  return track?.source === 'local' ? track.id : null;
};

/** Songs all here at once (an album's, a playlist's); `play(start)` plays them from row `start`. */
export function fixedData(
  engine: EngineClient,
  title: string,
  rows: { track: TrackRef; details?: LibraryTrack }[],
  play: (start: number) => void,
  empty: string,
): SongListData {
  return {
    title,
    count: rows.length,
    track: (i) => rows[i]?.track,
    details: (i) => rows[i]?.details,
    key: (i) => rows[i]?.track.id ?? `#${i}`,
    get currentKey() {
      return engine.current?.track.id ?? null;
    },
    play,
    error: (i) => (rows[i]?.details?.missing ? 'File missing' : (rows[i]?.details?.problem ?? null)),
    empty: { message: empty },
  };
}

/** Recently Played's songs, newest first; playing makes them the context, as listed. */
export function playedData(engine: EngineClient, plays: RecentPlays): SongListData {
  const row = (i: number) => plays.get(i);
  return {
    title: 'Recently Played',
    get count() {
      return plays.total;
    },
    track: (i) => row(i)?.track.track,
    details: (i) => row(i)?.track,
    key: (i) => {
      const r = row(i);
      return r ? `${r.track.track.id}@${r.played_at}` : `#${i}`;
    },
    get currentKey() {
      return localKey(engine);
    },
    play: async (start) => {
      // shortcut: fetches every play to make the context; a server-side "play history" query would send none.
      await plays.load(0, plays.total - 1);
      const tracks = Array.from({ length: plays.total }, (_, i) => row(i)?.track.track).filter((t) => t !== undefined);
      engine.run({ type: 'set_context', tracks, start, kind: 'list', label: 'Recently Played' });
    },
    get empty() {
      return { message: 'Nothing played yet.', hint: 'A song shows here once you have listened to half of it.' };
    },
    sync: () => plays.sync(),
    load: (from, to) => plays.load(from, to),
  };
}

export function trackListData(
  engine: EngineClient,
  library: LibraryClient,
  kind: ListSource,
  search: string | null = null,
  sort: SortRule[] = [],
  shown: Shown<LibraryTracks> = {},
): TrackListData {
  if (isDemo) return demoData(engine);
  return libraryData(engine, library, search, sort, shown);
}

/** Memoises a value computed from the data object's inputs. */
function lazy<T>(make: () => T): () => T {
  let value: { v: T } | undefined;
  return () => (value ??= { v: make() }).v;
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
    play: (start) => engine.run({ type: 'set_context', tracks: demoLibrary, start, kind: 'list', label: 'Library' }),
    playGroup: (g) =>
      engine.run({
        type: 'set_context',
        tracks: demoLibrary.slice(g.start, g.start + g.count),
        start: 0,
        kind: 'album',
        label: g.title,
      }),
    empty: { message: 'No demo tracks.' },
  };
}

function libraryData(
  engine: EngineClient,
  library: LibraryClient,
  search: string | null,
  sort: SortRule[],
  shown: Shown<LibraryTracks>,
): TrackListData {
  const next = library.tracks(search, sort);
  // Chosen on each read, so the swap follows `next.loaded` wherever the rows are drawn.
  const list = () => hold(next, shown);
  const run = (query: Parameters<EngineClient['query']>[0]) =>
    engine.query(query).catch((e: Error) => (engine.error = e.message));
  return {
    title: 'Library',
    kind: 'library',
    get order() {
      return list();
    },
    get count() {
      return list().total;
    },
    track: (i) => list().get(i)?.track,
    details: (i) => list().get(i),
    key: (i) => list().get(i)?.track.id ?? `#${i}`,
    get groups() {
      return list().groups;
    },
    get totalMs() {
      return list().groups.reduce((n, g) => n + g.duration_ms, 0);
    },
    get currentKey() {
      return localKey(engine);
    },
    play: (start) => void run({ type: 'play', search, sort, start }),
    playGroup: (g) => void run({ type: 'play_album', album: g.key, sort }),
    // The library keeps tracks whose files are gone or damaged, so they can
    // be found or replaced later; they show why they won't play.
    error: (i) => {
      const t = list().get(i);
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
    sync: () => list().sync(),
    load: (from, to) => list().load(from, to),
    anchor: (index, onShift) => {
      list().onShift = onShift;
      list().setAnchor(index);
    },
  };
}

