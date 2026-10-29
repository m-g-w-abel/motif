import { describe, expect, it } from 'vitest';
import type { LayoutNode } from './model';
import { fingerprint, idOf, insert, move, nodeAt, remove, resize, shownRoot, widget } from './overlay';
import { parseLayout } from './parse';
import { serializeLayout } from './serialize';

function rootOf(text: string): LayoutNode {
  const result = parseLayout(`layout version=1\n${text}`);
  if (!result.ok) throw new Error(result.errors.map((e) => e.message).join('; '));
  return result.layout.root;
}

/** A tree in the layout file's own words, on one line. */
function written(node: LayoutNode): string {
  const file = serializeLayout({ version: 1, name: 'x', description: '', icon: 'list', root: node });
  return file
    .split('\n')
    .slice(1)
    .map((l) => l.trim())
    .filter(Boolean)
    .join(' ');
}

const root = rootOf('split row { tracklist; split column size=300 { artwork; track-info } }');

describe('overlay', () => {
  it('shows the edited tree only while the file is the one it was made from', () => {
    const edited = remove(root, [0]);
    const overlay = { of: fingerprint(root), root: edited };
    expect(shownRoot(root, overlay)).toBe(edited);
    expect(shownRoot(rootOf('split row { tracklist; queue }'), overlay)).not.toBe(edited);
    expect(shownRoot(root, undefined)).toBe(root);
  });

  it('resizes a node and keeps the ids of everything rebuilt', () => {
    const resized = resize(root, [1, 0], { px: 120 });
    expect(nodeAt(resized, [1, 0])?.size).toEqual({ px: 120 });
    expect(idOf(resized)).toBe(idOf(root));
    expect(idOf(nodeAt(resized, [1])!)).toBe(idOf(nodeAt(root, [1])!));
    expect(resize(resized, [1, 0], undefined)).toEqual(root);
  });

  it('adds beside a widget into a split running that way, or wraps it in a new split', () => {
    expect(written(insert(root, [1, 0], 'below', widget('queue')))).toBe(
      'split row { tracklist split column size=300 { artwork queue track-info } }',
    );
    expect(written(insert(root, [1, 0], 'right', widget('queue')))).toBe(
      'split row { tracklist split column size=300 { split row { artwork queue } track-info } }',
    );
    // On a split that runs the same way, it goes at that end of the split.
    expect(written(insert(root, [], 'left', widget('queue')))).toBe(
      'split row { queue tracklist split column size=300 { artwork track-info } }',
    );
    // A layout of one widget wraps it, keeping its size on the new split.
    expect(written(insert(rootOf('tracklist size=400'), [], 'below', widget('controls')))).toBe(
      'split column size=400 { tracklist controls }',
    );
  });

  it('removes a node, and a split left with one widget becomes that widget', () => {
    expect(written(remove(root, [1, 0]))).toBe('split row { tracklist track-info size=300 }');
    expect(written(remove(remove(root, [1, 0]), [1]))).toBe('tracklist');
    expect(remove(rootOf('tracklist'), [])).toEqual(rootOf('tracklist'));
  });

  it('moves a node beside another anywhere in the tree', () => {
    expect(written(move(root, [1, 1], [0], 'left'))).toBe('split row { track-info tracklist artwork size=300 }');
    // The column is all that's left, so it fills the area: a top-level stack.
    expect(written(move(root, [0], [1, 1], 'below'))).toBe('artwork track-info tracklist');
    expect(written(move(root, [1, 0], [1, 1], 'below'))).toBe(
      'split row { tracklist split column size=300 { track-info artwork } }',
    );
    // Not into itself.
    expect(move(root, [1], [1, 0], 'right')).toBe(root);
  });

  it('round-trips an edited tree through a layout file', () => {
    const edited = move(insert(root, [0], 'above', widget('album-grid')), [1, 1], [0, 0], 'right');
    const back = parseLayout(serializeLayout({ version: 1, name: 'x', description: '', icon: 'list', root: edited }));
    expect(back.ok && back.layout.root).toEqual(edited);
  });
});
