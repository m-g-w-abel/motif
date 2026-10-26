import type { TrackGroup } from './protocol/TrackGroup';
import type { TrackRef } from './protocol/TrackRef';

/** Album artist and title, the way the library keys albums. */
export function albumKey(track: TrackRef): string {
  return `${track.artist ?? ''}\u001f${track.album ?? ''}`;
}

/**
 * Groups consecutive tracks that share an album and artist, the way a
 * playlist shows them; the same album appearing twice yields two groups.
 */
export function groupConsecutive(tracks: TrackRef[]): TrackGroup[] {
  const groups: TrackGroup[] = [];
  tracks.forEach((track, i) => {
    const key = albumKey(track);
    const last = groups.at(-1);
    if (last?.key === key) {
      last.count++;
      last.duration_ms += track.duration_ms ?? 0;
    } else {
      groups.push({
        key,
        start: i,
        count: 1,
        title: track.album ?? 'Unknown album',
        artist: track.artist ?? 'Unknown artist',
        duration_ms: track.duration_ms ?? 0,
      });
    }
  });
  return groups;
}
