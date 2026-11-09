import { describe, expect, it } from 'vitest';
import { devViews, Views, viewTitle } from './views.svelte';

const album = { kind: 'album', key: 'a1', title: 'Amnesiac' } as const;

describe('views', () => {
  it("know which of the phone bar's roots they are under: Library or Search", () => {
    const views = new Views();
    expect(views.root).toBe('library');
    views.open({ kind: 'albums' });
    views.open(album);
    expect(views.root).toBe('library');
    views.show({ kind: 'search' });
    views.open(album);
    expect(views.root).toBe('search');
    views.show({ kind: 'home' });
    expect(views.root).toBe('library');
  });

  it("close the phone bar's menu when going to another view", () => {
    const views = new Views();
    views.menu = 'engines';
    views.show({ kind: 'search' });
    expect(views.menu).toBeNull();
  });

  it('close the sort menu and the ⋮ sheet when going to another view', () => {
    const views = new Views();
    views.open({ kind: 'albums' });
    for (const go of [() => views.open(album), () => views.back(), () => views.show({ kind: 'playlists' })]) {
      views.sorting = true;
      views.options = true;
      go();
      expect([views.sorting, views.options]).toEqual([false, false]);
    }
  });

  it('start at the Library home, with nothing to go back to', () => {
    const views = new Views();
    expect(views.top).toEqual({ kind: 'home' });
    expect(views.parent).toBeNull();
    expect(views.back()).toBe(false);
  });

  it('go back down the stack: home → Albums → an album', () => {
    const views = new Views();
    views.open({ kind: 'albums' });
    views.open(album);
    expect(views.parent).toEqual({ kind: 'albums' });
    expect(views.back()).toBe(true);
    expect(views.top).toEqual({ kind: 'albums' });
    expect(views.back()).toBe(true);
    expect(views.top).toEqual({ kind: 'home' });
  });

  it('opening the view already on top twice adds it once (double taps, retries)', () => {
    const views = new Views();
    views.open(album);
    views.open({ ...album });
    views.back();
    expect(views.top).toEqual({ kind: 'home' });
  });

  it('a playlist opens once however often it is asked, and is told apart from others by id', () => {
    const views = new Views();
    views.open({ kind: 'playlists' });
    views.open({ kind: 'playlist', id: 'p1', name: 'Road' });
    views.open({ kind: 'playlist', id: 'p1', name: 'Road trip' });
    expect(views.parent).toEqual({ kind: 'playlists' });
    views.open({ kind: 'playlist', id: 'p2', name: 'Road' });
    expect(views.parent).toEqual({ kind: 'playlist', id: 'p1', name: 'Road' });
    expect(viewTitle(views.top)).toBe('Road');
  });

  it('show starts again from a category, as picking one in the desktop panel does', () => {
    const views = new Views();
    views.open({ kind: 'albums' });
    views.open(album);
    views.show({ kind: 'playlists' });
    expect(views.stack).toEqual([{ kind: 'home' }, { kind: 'playlists' }]);
    views.show({ kind: 'home' });
    expect(views.stack).toEqual([{ kind: 'home' }]);
  });

  it('a view at the bottom of the stack still goes back to the Library home', () => {
    const views = new Views();
    views.stack = [{ kind: 'search' }];
    expect(views.parent).toEqual({ kind: 'home' });
    expect(views.back()).toBe(true);
    expect(views.stack).toEqual([{ kind: 'home' }]);
  });

  it('are named for their back buttons', () => {
    expect(viewTitle({ kind: 'home' })).toBe('Library');
    expect(viewTitle({ kind: 'songs' })).toBe('All Songs');
    expect(viewTitle(album)).toBe('Amnesiac');
    expect(viewTitle({ kind: 'album', key: 'x', title: '' })).toBe('Unknown album');
  });
});

describe('dev-view', () => {
  it('opens a category or an album above the Library home', () => {
    expect(devViews('albums')).toEqual([{ kind: 'home' }, { kind: 'albums' }]);
    expect(devViews('album:a1')).toEqual([{ kind: 'home' }, { kind: 'albums' }, { kind: 'album', key: 'a1', title: 'a1' }]);
    expect(devViews('search')).toEqual([{ kind: 'home' }, { kind: 'search' }]);
  });

  it('ignores names it does not know', () => {
    expect(devViews('nope')).toBeNull();
    expect(devViews('album:')).toBeNull();
  });
});
