import { describe, expect, it } from 'vitest';
import type { SplitNode } from './model';
import { slots } from './overlay';
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
    store.move(store.root as SplitNode, 'split-row', 1, 0);
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

describe('LayoutStore add and remove', () => {
  const names = (store: LayoutStore) =>
    slots(store.root as SplitNode, 'split-row', store.edits).map((s) => (s.node.kind === 'split' ? 'split' : s.node.panel));

  it('adds, removes, moves and resets widgets', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=Edits\nsplit row { tracklist; artwork }\n', 'edits.kdl');
    store.move(store.root as SplitNode, 'split-row', 1, 0);
    store.addWidget(store.root as SplitNode, 'split-row', 'queue');
    store.removeWidget('split-row/1.artwork');
    expect(names(store)).toEqual(['tracklist', 'queue']);
    // Moving keeps the removed widget hidden.
    store.move(store.root as SplitNode, 'split-row', 1, 0);
    expect(names(store)).toEqual(['queue', 'tracklist']);
    store.resetEdits();
    expect(names(store)).toEqual(['tracklist', 'artwork']);
  });

  it('saves added and removed widgets with Save as…', () => {
    const store = new LayoutStore();
    store.select('builtin:tracks-art.kdl');
    const column = (store.root as SplitNode).children[1] as SplitNode;
    store.addWidget(column, 'split-row/1.split-column', 'queue');
    store.removeWidget('split-row/1.split-column/0.artwork');
    expect(store.saveAs('Added').ok).toBe(true);
    expect(store.active.text).toMatch(/split column size=300 \{\s*track-info\s*queue\s*\}/);
  });
});

describe('LayoutStore.saveAs', () => {
  it('writes the in-place edits into a new user layout and switches to it', () => {
    const store = new LayoutStore();
    store.select('builtin:tracks-art.kdl');
    store.move(store.root as SplitNode, 'split-row', 0, 1);
    store.resize('split-row/0.tracklist', { px: 500 });
    expect(store.saveAs('My art').ok).toBe(true);
    expect(store.activeId).toBe('user:my-art.kdl');
    expect(store.edited).toBe(false);
    const root = store.layout.root;
    const shown = root.kind === 'split' && root.children.map((c) => [c.kind === 'split' ? 'split' : c.panel, c.size]);
    expect(shown).toEqual([
      ['split', { px: 300 }],
      ['tracklist', { px: 500 }],
    ]);
  });

  it('refuses a name an installed user layout already has', () => {
    const store = new LayoutStore();
    expect(store.saveAs('Twice').ok).toBe(true);
    const before = store.user;
    const again = store.saveAs('twice!');
    expect(again.ok).toBe(false);
    expect(!again.ok && again.errors[0].message).toBe('a layout named “Twice” already exists');
    expect(store.user).toBe(before);
  });
});
