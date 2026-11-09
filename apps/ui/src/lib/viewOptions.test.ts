import { beforeEach, describe, expect, it, vi } from 'vitest';
import { SettingsStore } from './settings.svelte';
import { viewOptions } from './viewOptions';

const storage = new Map<string, string>();
vi.stubGlobal('localStorage', {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => storage.set(k, v),
  removeItem: (k: string) => storage.delete(k),
});

beforeEach(() => storage.clear());

describe('view options', () => {
  it('start as a cover grid for collections and a song list for a playlist', () => {
    const saved = new SettingsStore().current.viewOptions;
    expect(viewOptions(saved, 'albums')).toMatchObject({ mode: 'grid', columns: null, titles: true, artists: true });
    expect(viewOptions(saved, 'playlist:abc').mode).toBe('list');
  });

  it('start every song view as a song list, apart from its collection twin', () => {
    const saved = new SettingsStore().current.viewOptions;
    for (const key of ['songs', 'album', 'played:songs', 'added:songs'] as const) expect(viewOptions(saved, key).mode).toBe('list');
    expect(viewOptions(saved, 'added').mode).toBe('grid');
  });

  it('sort Albums by album artist, then year, and Recently Added newest first', () => {
    const saved = new SettingsStore().current.viewOptions;
    expect(viewOptions(saved, 'albums').sort).toEqual([
      { key: 'album_artist', descending: false },
      { key: 'year', descending: false },
    ]);
    expect(viewOptions(saved, 'added').sort?.[0]).toEqual({ key: 'added', descending: true });
    expect(viewOptions(saved, 'playlists').sort).toBeNull();
  });

  it('forget a sort reset to its default', () => {
    const s = new SettingsStore();
    s.setView('albums', { sort: [{ key: 'year', descending: true }] });
    expect(viewOptions(new SettingsStore().current.viewOptions, 'albums').sort).toEqual([{ key: 'year', descending: true }]);
    s.setView('albums', { sort: viewOptions({}, 'albums').sort });
    expect(s.current.viewOptions).toEqual({});
  });

  it('are remembered per kind of view across restarts', () => {
    const s = new SettingsStore();
    s.setView('albums', { mode: 'covers' });
    s.setView('albums', { columns: 5, artists: false });
    s.setView('playlists', { titles: false });
    const after = new SettingsStore().current.viewOptions;
    expect(viewOptions(after, 'albums')).toMatchObject({ mode: 'covers', columns: 5, titles: true, artists: false });
    expect(viewOptions(after, 'playlists')).toMatchObject({ mode: 'grid', titles: false, artists: true });
    expect(viewOptions(after, 'added').mode).toBe('grid');
  });

  it('are each playlist’s own', () => {
    const s = new SettingsStore();
    s.setView('playlist:a', { mode: 'grid', columns: 2 });
    const after = new SettingsStore().current.viewOptions;
    expect(viewOptions(after, 'playlist:a')).toMatchObject({ mode: 'grid', columns: 2 });
    expect(viewOptions(after, 'playlist:b')).toMatchObject({ mode: 'list', columns: null });
  });

  it('keep only what differs from the defaults', () => {
    const s = new SettingsStore();
    s.setView('albums', { mode: 'list', titles: false, sort: [{ key: 'year', descending: false }] });
    s.setView('albums', { mode: 'grid', titles: true, sort: viewOptions({}, 'albums').sort });
    expect(s.current.viewOptions).toEqual({});
  });

  it('ignore saved values that make no sense', () => {
    storage.set(
      'settings',
      JSON.stringify({
        viewOptions: {
          albums: { mode: 'carousel', columns: 0, titles: 'yes', sort: [{ key: 'colour', descending: false }] },
          played: { columns: 2.5, sort: { key: 'year', descending: true } },
          added: {
            columns: 99,
            sort: [{ key: 'year', descending: true }, { key: 'album', descending: 'no' }, { key: 'album', descending: false }],
          },
          playlists: 'grid',
        },
      }),
    );
    const saved = new SettingsStore().current.viewOptions;
    expect(viewOptions(saved, 'albums')).toEqual({ ...viewOptions({}, 'albums') });
    expect(viewOptions(saved, 'played')).toMatchObject({ columns: null, sort: null });
    expect(viewOptions(saved, 'added')).toMatchObject({
      columns: null,
      sort: [
        { key: 'year', descending: true },
        { key: 'album', descending: false },
      ],
    });
    expect(viewOptions(saved, 'playlists').mode).toBe('grid');
  });
});
