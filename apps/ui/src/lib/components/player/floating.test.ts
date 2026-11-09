import { describe, expect, it } from 'vitest';
import { isFloatPos, place, resolve, snap, type FloatBox } from './floating';

const b: FloatBox = { w: 300, h: 100, W: 1000, H: 600 };

describe('floating player maths', () => {
  it('anchors to the nearest corner', () => {
    expect(place(30, 40, b)).toEqual({ h: 'left', v: 'top', x: 30, y: 40 });
    expect(place(670, 460, b)).toEqual({ h: 'right', v: 'bottom', x: 30, y: 40 });
  });

  it('keeps a 16px margin from every edge', () => {
    expect(place(-50, -50, b)).toEqual({ h: 'left', v: 'top', x: 16, y: 16 });
    expect(place(5000, 5000, b)).toEqual({ h: 'right', v: 'bottom', x: 16, y: 16 });
  });

  it('snaps onto the margin within 24px of it, and not beyond', () => {
    expect(snap({ h: 'left', v: 'top', x: 40, y: 41 }, b)).toEqual({ h: 'left', v: 'top', x: 16, y: 41 });
    expect(snap({ h: 'right', v: 'bottom', x: 50, y: 20 }, b)).toEqual({ h: 'right', v: 'bottom', x: 50, y: 16 });
  });

  it('keeps its edge offset when the window resizes, and stays on screen', () => {
    const p = { h: 'right', v: 'bottom', x: 16, y: 16 } as const;
    expect(resolve(p, { ...b, W: 800, H: 500 })).toEqual({ left: 484, top: 384 });
    expect(resolve({ h: 'right', v: 'bottom', x: 900, y: 16 }, { ...b, W: 400 })).toEqual({ left: 16, top: 484 });
  });

  it('rejects the old top-left format', () => {
    expect(isFloatPos({ x: 10, y: 20 })).toBe(false);
    expect(isFloatPos({ h: 'left', v: 'top', x: 1, y: 2 })).toBe(true);
  });
});
