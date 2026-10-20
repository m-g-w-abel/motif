import { demoLibrary } from './demo';
import type { EngineClient } from './engine.svelte';
/** A list of tracks the widgets can show: the browsed library, or the play queue. */
export type ListSource = 'library' | 'queue';
import type { TrackInfo } from './protocol/TrackInfo';

/** A list of tracks from one source, with what playing or removing means for it. */
export interface TrackListData {
  title: string;
  kind: ListSource;
  tracks: TrackInfo[];
  /** Stable per-row keys. */
  keys: string[];
  /** Index of the playing track in `tracks`, or -1. */
  currentIndex: number;
  play(index: number): void;
  /** Plays a subset (e.g. one album) from its first track. */
  playIndices(indices: number[]): void;
  remove?(index: number): void;
}

export function trackListData(engine: EngineClient, kind: ListSource): TrackListData {
  if (kind === 'library') {
    const currentUri = engine.current?.track.uri;
    return {
      title: 'Library',
      kind,
      tracks: demoLibrary,
      keys: demoLibrary.map((t) => t.uri),
      currentIndex: demoLibrary.findIndex((t) => t.uri === currentUri),
      play: (start) => engine.run({ type: 'set_queue', tracks: demoLibrary, start }),
      playIndices: (indices) =>
        engine.run({ type: 'set_queue', tracks: indices.map((i) => demoLibrary[i]), start: 0 }),
    };
  }
  const queue = engine.player?.queue ?? [];
  return {
    title: 'Queue',
    kind,
    tracks: queue.map((item) => item.track),
    keys: queue.map((item) => String(item.id)),
    currentIndex: engine.player?.current ?? -1,
    play: (i) => engine.run({ type: 'play_item', item_id: queue[i].id }),
    playIndices: (indices) => {
      if (indices.length) engine.run({ type: 'play_item', item_id: queue[indices[0]].id });
    },
    remove: (i) => engine.run({ type: 'remove_item', item_id: queue[i].id }),
  };
}
