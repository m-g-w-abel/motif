import { describe, expect, it } from 'vitest';
import type { TrackRef } from '../../protocol/TrackRef';
import { albumOf, keptArt } from './artwork';

const track = (artist: string | null, album: string | null) => ({ artist, album }) as TrackRef;
const shown = { id: 'a1', album: albumOf(track('Machine Girl', 'Gemini'))! };

describe('keptArt', () => {
  it('keeps the image for another track of the same album', () => {
    expect(keptArt(shown, track('Machine Girl', 'Gemini'), null)).toBe('a1');
  });
  it('keeps any image while the next art is still on its way', () => {
    expect(keptArt(shown, track('Other', 'Album'), 'b1', true)).toBe('a1');
    expect(keptArt(shown, track('Other', 'Album'), null, true)).toBe('a1');
    expect(keptArt(shown, track('A', null), null, true)).toBe('a1');
    expect(keptArt(null, track('Other', 'Album'), 'b1', true)).toBeNull();
  });
  it('drops it for another album, even by the same artist, once its art is known', () => {
    expect(keptArt(shown, track('Machine Girl', 'WLFGRL'), 'b1')).toBeNull();
    expect(keptArt(shown, track('Other', 'Gemini'), null)).toBeNull();
  });
  it('never keeps an image across tracks without an album', () => {
    expect(keptArt({ id: 'x', album: '' }, track('A', null), 'y')).toBeNull();
    expect(keptArt(null, track('A', 'B'), 'a1')).toBeNull();
    expect(keptArt(shown, undefined, null)).toBeNull();
  });
  it('shows its own image without a track or album', () => {
    expect(keptArt(shown, undefined, 'a1')).toBe('a1');
    expect(keptArt({ id: 'x', album: '' }, track('A', null), 'x')).toBe('x');
  });
});
