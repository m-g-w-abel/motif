import { describe, expect, it } from 'vitest';
import { LayoutStore, withName } from './store.svelte';

describe('withName', () => {
  it('replaces an existing name and keeps comments', () => {
    const text = '// mine\nlayout version=1 name=Standard\nview v { queue }\n';
    expect(withName(text, 'Standard (custom)')).toBe(
      '// mine\nlayout version=1 name="Standard (custom)"\nview v { queue }\n',
    );
  });

  it('adds a name when there is none', () => {
    expect(withName('layout version=1\n', 'New')).toBe('layout version=1 name=New\n');
  });
});

describe('LayoutStore', () => {
  const userNames = (store: LayoutStore) => store.user.map((e) => e.layout.name);

  it('installs, renames and removes a user layout', () => {
    const store = new LayoutStore();
    const installed = store.install('layout version=1 name=Mine\ntracklist\n', 'whatever.kdl');
    expect(installed.ok).toBe(true);
    expect(userNames(store)).toEqual(['Mine']);
    expect(store.activeId).toBe('user:mine.kdl');

    expect(store.saveEdit('layout version=1 name="Mine 2"\ntracklist\n').ok).toBe(true);
    expect(userNames(store)).toEqual(['Mine 2']);
    expect(store.activeId).toBe('user:mine-2.kdl');

    store.remove(store.activeId);
    expect(userNames(store)).toEqual([]);
    expect(store.active.builtin).toBe(true);
  });

  it('saves edits to a built-in as a custom copy', () => {
    const store = new LayoutStore();
    const builtin = store.active;
    expect(store.saveEdit(builtin.text).ok).toBe(true);
    expect(userNames(store)).toEqual([`${builtin.layout.name} (custom)`]);
    expect(store.builtins).toContain(builtin);
  });
});

describe('LayoutStore edits', () => {
  it('resizes, reorders and resets the layout on screen', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=Edits\nsplit row { tracklist; artwork size=200 }\n', 'edits.kdl');
    store.resize('split-row/1.artwork', { px: 320 });
    store.move('split-row', 2, 1, 0);
    expect(store.edits).toEqual({ sizes: { 'split-row/1.artwork': { px: 320 } }, order: { 'split-row': [1, 0] } });
    expect(store.edited).toBe(true);
    store.resetEdits();
    expect(store.edited).toBe(false);
  });

  it('drops edits that no longer fit after the file changes', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=Edits\nsplit row { tracklist; artwork }\n', 'edits.kdl');
    store.resize('split-row/0.tracklist', { px: 400 });
    store.resize('split-row/1.artwork', { px: 300 });
    store.saveEdit('layout version=1 name=Edits\nsplit row { tracklist; queue }\n');
    expect(store.edits).toEqual({ sizes: { 'split-row/0.tracklist': { px: 400 } } });
  });
});
