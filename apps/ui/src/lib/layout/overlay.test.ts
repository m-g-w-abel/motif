import { describe, expect, it } from 'vitest';
import type { SplitNode } from './model';
import { applyOverlay, prune, rootPath, slots, type Overlay } from './overlay';
import { parseLayout } from './parse';
import { serializeLayout } from './serialize';

function rootOf(text: string): SplitNode {
  const result = parseLayout(`layout version=1\n${text}`);
  if (!result.ok || result.layout.root.kind !== 'split') throw new Error('expected a split');
  return result.layout.root;
}

const root = rootOf('split row { tracklist; split column size=300 { artwork; track-info } }');
const shown = (split: SplitNode, path: string, overlay = {}) =>
  slots(split, path, overlay).map((s) => [s.path, s.size]);

describe('overlay', () => {
  it('names nodes along the path', () => {
    expect(rootPath(root)).toBe('split-row');
    expect(shown(root, 'split-row')).toEqual([
      ['split-row/0.tracklist', undefined],
      ['split-row/1.split-column', { px: 300 }],
    ]);
  });

  it('applies sizes and order', () => {
    const overlay = {
      sizes: { 'split-row/1.split-column': { percent: 40 }, 'split-row/0.tracklist': { px: 500 } },
      order: { 'split-row': [1, 0] },
    };
    expect(shown(root, 'split-row', overlay)).toEqual([
      ['split-row/1.split-column', { percent: 40 }],
      ['split-row/0.tracklist', { px: 500 }],
    ]);
  });

  it('ignores an order that no longer fits the split', () => {
    expect(shown(root, 'split-row', { order: { 'split-row': [2, 0, 1] } }).map((s) => s[0])).toEqual([
      'split-row/0.tracklist',
      'split-row/1.split-column',
    ]);
  });

  it('prunes paths that are gone and orders that change nothing', () => {
    const overlay = {
      sizes: { 'split-row/1.split-column': { px: 250 }, 'split-row/1.queue': { px: 100 } },
      order: { 'split-row': [0, 1], 'split-row/1.split-column': [1, 0] },
    };
    expect(prune(root, overlay)).toEqual({
      sizes: { 'split-row/1.split-column': { px: 250 } },
      order: { 'split-row/1.split-column': [1, 0] },
    });
    // The second widget changed, so its edits no longer apply.
    expect(prune(rootOf('split row { tracklist; queue }'), overlay)).toEqual({ sizes: { 'split-row/1.queue': { px: 100 } } });
    expect(prune(rootOf('split column { tracklist; queue }'), overlay)).toEqual({});
  });

  const edited: Overlay = {
    added: { 'split-row': ['queue'], 'split-row/1.split-column': ['controls'] },
    removed: ['split-row/1.split-column/0.artwork'],
    order: { 'split-row': [2, 0, 1] },
    sizes: { 'split-row/2.queue': { px: 200 } },
  };

  it('adds widgets after the file children and hides removed ones', () => {
    expect(shown(root, 'split-row', edited)).toEqual([
      ['split-row/2.queue', { px: 200 }],
      ['split-row/0.tracklist', undefined],
      ['split-row/1.split-column', { px: 300 }],
    ]);
    const column = root.children[1] as SplitNode;
    expect(shown(column, 'split-row/1.split-column', edited).map((s) => s[0])).toEqual([
      'split-row/1.split-column/1.track-info',
      'split-row/1.split-column/2.controls',
    ]);
  });

  it('keeps additions and removals that still fit', () => {
    expect(prune(root, edited)).toEqual(edited);
    // The column became a row: its addition and the removal inside it no longer apply.
    expect(prune(rootOf('split row { tracklist; split row { artwork; track-info } }'), edited)).toEqual({
      added: { 'split-row': ['queue'] },
      order: { 'split-row': [2, 0, 1] },
      sizes: { 'split-row/2.queue': { px: 200 } },
    });
  });

  it('writes the edits into the tree, dropping emptied splits, and round-trips through a file', () => {
    const tree = applyOverlay(root, { ...edited, removed: [...edited.removed!, 'split-row/1.split-column/1.track-info'] });
    const file = serializeLayout({ version: 1, name: 'Edited', description: '', icon: 'list', root: tree });
    expect(file).toContain('queue size=200');
    const back = parseLayout(file);
    expect(back.ok && back.layout.root).toEqual(tree);
    expect(back.ok && JSON.stringify(back.layout.root)).toMatch(/"queue".*"tracklist".*"controls"/);
    expect(applyOverlay(root, { removed: ['split-row/1.split-column/0.artwork', 'split-row/1.split-column/1.track-info'] })).toEqual({
      ...root,
      children: [root.children[0]],
    });
  });
});
