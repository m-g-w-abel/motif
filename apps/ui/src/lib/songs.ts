import { formatTime, trackFormat } from './format';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { TrackRef } from './protocol/TrackRef';

/** A song row's right-hand text: "disc · length · format" on an album page, else the length. */
export function songInfo(track: TrackRef, details: LibraryTrack | undefined, inAlbum: boolean): string {
  const length = track.duration_ms == null ? '' : formatTime(track.duration_ms);
  if (!inAlbum) return length;
  return [details?.disc_no, length, trackFormat(track)].filter((part) => part != null && part !== '').join(' · ');
}

/** A song row's number: its track number on an album page, else its place in the list. */
export const songNumber = (index: number, details: LibraryTrack | undefined, inAlbum: boolean) =>
  (inAlbum && details?.track_no) || index + 1;

/** Whether a song matches an in-page search (an album's or a playlist's). */
export function matches(track: TrackRef, query: string): boolean {
  const q = query.trim().toLowerCase();
  return [track.title, track.artist, track.album].some((field) => field?.toLowerCase().includes(q)) || !q;
}

/** An album page's line under the title: "album artist · year". */
export function albumLine(track: TrackRef, details: LibraryTrack | undefined): string {
  const artist = details?.album_artist ?? track.artist ?? 'Unknown artist';
  const year = details?.original_year ?? details?.year;
  return year ? `${artist} · ${year}` : artist;
}
