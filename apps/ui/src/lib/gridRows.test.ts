import { describe, expect, it } from 'vitest';
import { gridRows } from './gridRows';

describe('gridRows', () => {
  it('draws rows within the overscan above and below the view', () => {
    // 200px rows, a 800px view 1000px in: rows 5-9 show; 600px more each side.
    expect(gridRows(1000, 800, 200, 100, 600)).toEqual({ first: 2, last: 12 });
  });

  it('stops at the first and last row', () => {
    expect(gridRows(0, 800, 200, 100, 600)).toEqual({ first: 0, last: 7 });
    expect(gridRows(19000, 800, 200, 100, 600)).toEqual({ first: 92, last: 100 });
  });

  it('draws further than the old row either side', () => {
    const { first, last } = gridRows(1000, 800, 200, 100);
    expect(first).toBeLessThan(4);
    expect(last).toBeGreaterThan(11);
  });
});
