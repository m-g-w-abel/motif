import { describe, expect, it } from 'vitest';
import type { Column } from './layout/model';
import { fitColumns, flexReserve, nearest, visibleRange } from './table';

describe('fitColumns', () => {
  const shown: Column[] = ['index', 'title', 'artist', 'album', 'duration'];
  const sum = (w: Partial<Record<Column, number>>) => Object.values(w).reduce((n, x) => n + (x ?? 0), 0);

  it('keeps requested widths when they fit or the table is unmeasured', () => {
    const wide = { index: 44, artist: 190, album: 210, duration: 64 };
    expect(fitColumns({}, shown, 'title', 1400, flexReserve(1400))).toEqual(wide);
    expect(fitColumns({}, shown, 'title', 0, flexReserve(0))).toEqual(wide);
  });

  it('shrinks wide columns so a narrow panel keeps room for the title', () => {
    const fitted = fitColumns({}, shown, 'title', 400, flexReserve(400));
    expect(fitted.index).toBe(44);
    expect(fitted.duration).toBe(64);
    expect(400 - sum(fitted)).toBeGreaterThanOrEqual(flexReserve(400) - 1);
  });
});

describe('visibleRange', () => {
  const tops = [0, 26, 52, 84, 110, 136];

  it('includes rows partly in view', () => {
    expect(visibleRange(tops, 30, 90)).toEqual([1, 4]);
    expect(visibleRange(tops, 0, 26)).toEqual([0, 1]);
  });

  it('handles the ends and an empty list', () => {
    expect(visibleRange(tops, 500, 600)).toEqual([5, 6]);
    expect(visibleRange(tops, -100, 10)).toEqual([0, 1]);
    expect(visibleRange([], 0, 100)).toEqual([0, 0]);
  });
});

describe('nearest', () => {
  it('picks the closest point, the lower one on a tie', () => {
    expect(nearest([0, 100, 300], 40)).toBe(0);
    expect(nearest([0, 100, 300], 60)).toBe(100);
    expect(nearest([0, 100, 300], 200)).toBe(100);
    expect(nearest([0, 100, 300], 900)).toBe(300);
    expect(nearest([0, 100, 300], -5)).toBe(0);
    expect(nearest([], 42)).toBe(42);
  });
});
