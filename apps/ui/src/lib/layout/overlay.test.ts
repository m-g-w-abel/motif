import { describe, expect, it } from 'vitest';
import type { SplitNode } from './model';
import { prune, rootPath, slots } from './overlay';
import { parseLayout } from './parse';

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
});
