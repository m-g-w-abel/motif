import { describe, expect, it } from 'vitest';
import { dropIndex, keyTarget, moveItem, shift } from './reorder';

describe('reorder', () => {
  it('moves an item', () => {
    expect(moveItem(['a', 'b', 'c'], 0, 2)).toEqual(['b', 'c', 'a']);
    expect(moveItem(['a', 'b', 'c'], 2, 0)).toEqual(['c', 'a', 'b']);
    expect(moveItem(['a', 'b', 'c'], 1, 1)).toEqual(['a', 'b', 'c']);
  });
  it('finds the drop index from the other rows', () => {
    const mids = [10, 30, 50];
    expect(dropIndex(mids, 0, 0)).toBe(0);
    expect(dropIndex(mids, 0, 41)).toBe(1);
    expect(dropIndex(mids, 0, 99)).toBe(2);
    expect(dropIndex(mids, 2, 0)).toBe(0);
    expect(dropIndex(mids, 2, 20)).toBe(1);
  });
  it('shifts the rows between', () => {
    expect([0, 1, 2, 3].map((i) => shift(i, 0, 2))).toEqual([0, -1, -1, 0]);
    expect([0, 1, 2, 3].map((i) => shift(i, 3, 1))).toEqual([0, 1, 1, 0]);
  });
  it('maps Alt+Arrow and stops at the ends', () => {
    expect(keyTarget({ key: 'ArrowUp', altKey: true }, 1, 3)).toBe(0);
    expect(keyTarget({ key: 'ArrowUp', altKey: true }, 0, 3)).toBeNull();
    expect(keyTarget({ key: 'ArrowDown', altKey: true }, 2, 3)).toBeNull();
    expect(keyTarget({ key: 'ArrowDown', altKey: false }, 0, 3)).toBeNull();
  });
});
