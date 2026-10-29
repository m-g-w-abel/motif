import { describe, expect, it } from 'vitest';
import type { LayoutNode } from './model';
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

const names = (node: LayoutNode): unknown =>
  node.kind === 'split' ? { [node.direction]: node.children.map(names), ...(node.size ? { size: node.size } : {}) } : node.panel;

describe('LayoutStore edits', () => {
  it('resizes, moves, adds, removes and resets the layout on screen', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=Edits\nsplit row { tracklist; artwork size=200 }\n', 'edits.kdl');
    store.resize([1], { px: 320 });
    store.move([1], [0], 'left');
    expect(names(store.shown)).toEqual({ row: ['artwork', 'tracklist'] });
    expect(store.shown.kind === 'split' && store.shown.children[0].size).toEqual({ px: 320 });
    store.addWidget([1], 'below', 'queue');
    store.removeWidget([0]);
    expect(names(store.shown)).toEqual({ column: ['tracklist', 'queue'] });
    expect(store.edited).toBe(true);
    store.resetEdits();
    expect(store.edited).toBe(false);
    expect(store.shown).toBe(store.root);
  });

  it('adds to a layout of one widget', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=One\ntracklist\n', 'one.kdl');
    store.addWidget([], 'right', 'artwork');
    expect(names(store.shown)).toEqual({ row: ['tracklist', 'artwork'] });
  });

  it('drops edits once the file changes', () => {
    const store = new LayoutStore();
    store.install('layout version=1 name=Edits\nsplit row { tracklist; artwork }\n', 'edits.kdl');
    store.resize([0], { px: 400 });
    store.saveEdit('layout version=1 name=Edits\nsplit row { tracklist; queue }\n');
    expect(store.edited).toBe(false);
  });
});

describe('LayoutStore.saveAs', () => {
  it('writes the in-place edits into a new user layout and switches to it', () => {
    const store = new LayoutStore();
    store.select('builtin:tracks-art.kdl');
    store.move([0], [1], 'right');
    store.resize([1], { px: 500 });
    store.addWidget([0, 1], 'below', 'queue');
    expect(store.saveAs('My art').ok).toBe(true);
    expect(store.activeId).toBe('user:my-art.kdl');
    expect(store.edited).toBe(false);
    expect(names(store.layout.root)).toEqual({
      row: [{ column: ['artwork', 'track-info', 'queue'], size: { px: 300 } }, 'tracklist'],
    });
    expect(store.layout.root.kind === 'split' && store.layout.root.children[1].size).toEqual({ px: 500 });
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
