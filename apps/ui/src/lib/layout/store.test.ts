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
