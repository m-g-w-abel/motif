import { describe, expect, it } from 'vitest';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { TrackRef } from './protocol/TrackRef';
import { albumLine, matches, songInfo, songNumber } from './songs';

const track: TrackRef = {
  source: 'local',
  id: '/music/Aphex Twin/SAW/06 Green Calx.opus',
  title: 'Green Calx',
  artist: 'Aphex Twin',
  album: 'Selected Ambient Works 85–92',
  duration_ms: 365_000,
} as TrackRef;

const details = (patch: Partial<LibraryTrack>): LibraryTrack =>
  ({ id: 1, track, album_artist: null, track_no: null, disc_no: null, year: null, original_year: null, ...patch }) as LibraryTrack;

describe('song rows', () => {
  it('show disc, length and format on an album page, just the length elsewhere', () => {
    expect(songInfo(track, details({ disc_no: 1 }), true)).toBe('1 · 6:05 · OPUS');
    expect(songInfo(track, details({}), true)).toBe('6:05 · OPUS');
    expect(songInfo(track, details({ disc_no: 2 }), false)).toBe('6:05');
  });

  it('number album rows by track number, other lists by position', () => {
    expect(songNumber(0, details({ track_no: 6 }), true)).toBe(6);
    expect(songNumber(0, details({}), true)).toBe(1);
    expect(songNumber(4, details({ track_no: 6 }), false)).toBe(5);
  });

  it('match a search on title, artist or album, ignoring case', () => {
    expect(matches(track, 'calx')).toBe(true);
    expect(matches(track, 'APHEX')).toBe(true);
    expect(matches(track, 'ambient')).toBe(true);
    expect(matches(track, 'radiohead')).toBe(false);
    expect(matches(track, '')).toBe(true);
  });
});

describe('album header', () => {
  it('reads "album artist · year", preferring the original year', () => {
    expect(albumLine(track, details({ album_artist: 'AFX', year: 2002, original_year: 1992 }))).toBe('AFX · 1992');
    expect(albumLine(track, details({ year: 1992 }))).toBe('Aphex Twin · 1992');
    expect(albumLine(track, undefined)).toBe('Aphex Twin');
  });
});
