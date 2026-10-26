import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SortRule } from './protocol/SortRule';
import { DEFAULT_RULES, effectiveSort, nextOneOff, sortLabel, SortStore } from './sort.svelte';

const storage = new Map<string, string>();
vi.stubGlobal('localStorage', {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => storage.set(k, v),
  removeItem: (k: string) => storage.delete(k),
});

const byYear: SortRule[] = [
  { key: 'year', descending: true },
  { key: 'album', descending: false },
];

describe('SortStore', () => {
  beforeEach(() => storage.clear());

  it('starts with the default order', () => {
    expect(new SortStore().rules.library).toEqual(DEFAULT_RULES);
  });

  it('saves rules per view on this device', () => {
    new SortStore().set('library', byYear);
    expect(new SortStore().rules.library).toEqual(byYear);
    new SortStore().set('library', []);
    expect(storage.size).toBe(0);
    expect(new SortStore().rules.library).toEqual(DEFAULT_RULES);
  });

  it('ignores saved rules it does not understand', () => {
    storage.set(
      'sort.library',
      JSON.stringify([
        { key: 'mood', descending: false },
        { key: 'year', descending: true },
      ]),
    );
    expect(new SortStore().rules.library).toEqual([{ key: 'year', descending: true }]);
    storage.set('sort.library', 'not json');
    expect(new SortStore().rules.library).toEqual(DEFAULT_RULES);
  });
});

describe('column header sort', () => {
  it('sorts, reverses, then restores the saved rules', () => {
    const first = nextOneOff(null, 'title');
    expect(first).toEqual({ key: 'title', descending: false });
    const second = nextOneOff(first, 'title');
    expect(second).toEqual({ key: 'title', descending: true });
    expect(nextOneOff(second, 'title')).toBeNull();
    expect(nextOneOff(second, 'artist')).toEqual({
      key: 'artist',
      descending: false,
    });
  });

  it('goes first, with the saved rules breaking ties, and leaves them as they were', () => {
    const rules = [...byYear];
    expect(effectiveSort(rules, { key: 'album', descending: true })).toEqual([
      { key: 'album', descending: true },
      { key: 'year', descending: true },
    ]);
    expect(effectiveSort(rules, null)).toEqual(byYear);
    expect(rules).toEqual(byYear);
  });
});

describe('sortLabel', () => {
  it('shortens values to initials and numbers', () => {
    expect(sortLabel('artist', { artist: 'björk' })).toBe('B');
    expect(sortLabel('album_artist', { album_artist: null, artist: 'Ostra' })).toBe('O');
    expect(sortLabel('title', { title: '99 Problems' })).toBe('#');
    expect(sortLabel('genre', { genre: null })).toBe('–');
    expect(sortLabel('year', { year: 1997 })).toBe('1997');
    expect(sortLabel('duration', { duration_ms: 185_000 })).toBe('3 min');
    expect(sortLabel('added', {})).toBeUndefined();
  });
});
