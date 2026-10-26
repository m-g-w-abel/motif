import { demoLibrary } from './demo';
import type { EngineClient } from './engine.svelte';
/** A list of tracks the widgets can show: the browsed library, or the play queue. */
export type ListSource = 'library' | 'queue';
import type { TrackRef } from './protocol/TrackRef';

/** A list of tracks from one source, with what playing or removing means for it. */
export interface TrackListData {
  title: string;
  kind: ListSource;
  tracks: TrackRef[];
  /** Stable per-row keys. */
  keys: string[];
  /** Why a row can't be played, where the engine has tried and failed. */
  errors?: (string | null)[];
  /** Index of the playing track in `tracks`, or -1. A getter, so lists stay put when only playback changes. */
  readonly currentIndex: number;
  play(index: number): void;
  /** Plays a subset (e.g. one album) from its first track. */
  playIndices(indices: number[]): void;
  remove?(index: number): void;
}

const libraryKeys = demoLibrary.map((t) => t.id);

export function trackListData(engine: EngineClient, kind: ListSource): TrackListData {
  if (kind === 'library') {
    return {
      title: 'Library',
      kind,
      tracks: demoLibrary,
      keys: libraryKeys,
      get currentIndex() {
        const id = engine.current?.track.id;
        return demoLibrary.findIndex((t) => t.id === id);
      },
      play: (start) => engine.run({ type: 'set_queue', tracks: demoLibrary, start }),
      playIndices: (indices) =>
        engine.run({ type: 'set_queue', tracks: indices.map((i) => demoLibrary[i]), start: 0 }),
    };
  }
  const queue = engine.queue;
  return {
    title: 'Queue',
    kind,
    tracks: queue.map((item) => item.track),
    keys: queue.map((item) => String(item.id)),
    errors: queue.map((item) => item.error),
    get currentIndex() {
      return engine.player?.current ?? -1;
    },
    play: (i) => engine.run({ type: 'play_item', item_id: queue[i].id }),
    playIndices: (indices) => {
      if (indices.length) engine.run({ type: 'play_item', item_id: queue[indices[0]].id });
    },
    remove: (i) => engine.run({ type: 'remove_item', item_id: queue[i].id }),
  };
}
