import { describe, expect, it } from 'vitest';
import { flip, motion } from './screens';

describe('motion', () => {
  it('goes forward into a deeper screen, back out of one, and swaps between screens at the same depth', () => {
    expect(motion(1, 2)).toBe('forward');
    expect(motion(3, 2)).toBe('back');
    expect(motion(2, 2)).toBe('swap');
  });
});

describe('flip', () => {
  const box = (left: number, top: number, width: number, height: number) => ({ left, top, width, height });

  it('puts the new box over the old one, scaling each side (origin top left)', () => {
    expect(flip(box(10, 20, 100, 50), box(0, 0, 200, 200))).toEqual({ dx: 10, dy: 20, sx: 0.5, sy: 0.25 });
  });

  it('scales text evenly by the given ratio, so it keeps its shape', () => {
    expect(flip(box(10, 20, 100, 16), box(30, 40, 300, 48), 0.5)).toEqual({ dx: -20, dy: -20, sx: 0.5, sy: 0.5 });
  });
});
