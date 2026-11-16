import { beforeEach, describe, expect, it, vi } from 'vitest';
import { lastArtPixels, rememberArtPixels } from './artMemory';

const storage = new Map<string, string>();
vi.stubGlobal('localStorage', {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => storage.set(k, v),
  removeItem: (k: string) => storage.delete(k),
});

beforeEach(() => storage.clear());

describe('remembered art colours', () => {
  it('give back the last cover pixels, so a restart starts in them', () => {
    const pixels = Uint8ClampedArray.from({ length: 16 * 16 * 4 }, (_, i) => i % 256);
    rememberArtPixels(pixels);
    expect(lastArtPixels()).toEqual(pixels);
  });

  it('are forgotten when the track has no colours', () => {
    rememberArtPixels(new Uint8ClampedArray(1024));
    rememberArtPixels(null);
    expect(lastArtPixels()).toBeNull();
  });

  it('are null when nothing or something broken was saved', () => {
    expect(lastArtPixels()).toBeNull();
    for (const bad of ['not json', '{"a":1}', '[1,2,3]', JSON.stringify(Array(1024).fill('x'))]) {
      storage.set('art-pixels', bad);
      expect(lastArtPixels()).toBeNull();
    }
  });

  it('are null when storage is unavailable', () => {
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } });
    expect(() => rememberArtPixels(new Uint8ClampedArray(1024))).not.toThrow();
    expect(lastArtPixels()).toBeNull();
  });
});
