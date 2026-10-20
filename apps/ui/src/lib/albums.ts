import type { TrackInfo } from './protocol/TrackInfo';

export interface Album {
  key: string;
  title: string;
  artist: string;
  /** Indices into the source track list, in order. */
  indices: number[];
  durationMs: number;
}

export function albumKey(track: TrackInfo): string {
  return `${track.artist ?? ''}\u0000${track.album ?? ''}`;
}

/**
 * Groups consecutive tracks that share an album and artist, the way a
 * playlist shows them; the same album appearing twice yields two groups.
 */
export function groupConsecutive(tracks: TrackInfo[]): Album[] {
  const groups: Album[] = [];
  tracks.forEach((track, i) => {
    const key = albumKey(track);
    const last = groups.at(-1);
    if (last?.key === key) {
      last.indices.push(i);
      last.durationMs += track.duration_ms ?? 0;
    } else {
      groups.push({
        key,
        title: track.album ?? 'Unknown album',
        artist: track.artist ?? 'Unknown artist',
        indices: [i],
        durationMs: track.duration_ms ?? 0,
      });
    }
  });
  return groups;
}

/** One entry per distinct album, in order of first appearance. */
export function distinctAlbums(tracks: TrackInfo[]): Album[] {
  const byKey = new Map<string, Album>();
  for (const group of groupConsecutive(tracks)) {
    const existing = byKey.get(group.key);
    if (existing) {
      existing.indices.push(...group.indices);
      existing.durationMs += group.durationMs;
    } else {
      byKey.set(group.key, { ...group, indices: [...group.indices] });
    }
  }
  return [...byKey.values()];
}
