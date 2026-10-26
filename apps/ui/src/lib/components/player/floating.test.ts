import { describe, expect, it } from 'vitest';
import { isFloatPos, place, resolve, snap, type FloatBox } from './floating';

const b: FloatBox = { w: 300, h: 100, W: 1000, H: 600, minTop: 0 };

describe('floating player maths', () => {
  it('anchors to the nearest corner', () => {
    expect(place(10, 20, b)).toEqual({ h: 'left', v: 'top', x: 10, y: 20 });
    expect(place(690, 480, b)).toEqual({
      h: 'right',
      v: 'bottom',
      x: 10,
      y: 20,
    });
  });

  it('clamps on screen and respects minTop', () => {
    expect(place(-50, -50, { ...b, minTop: 56 })).toEqual({
      h: 'left',
      v: 'top',
      x: 0,
      y: 56,
    });
    expect(place(5000, 5000, b)).toEqual({
      h: 'right',
      v: 'bottom',
      x: 0,
      y: 0,
    });
  });

  it('snaps within 24px of an edge and not beyond', () => {
    expect(snap({ h: 'left', v: 'top', x: 24, y: 25 }, b)).toEqual({
      h: 'left',
      v: 'top',
      x: 0,
      y: 25,
    });
    expect(snap({ h: 'right', v: 'bottom', x: 30, y: 3 }, b)).toEqual({
      h: 'right',
      v: 'bottom',
      x: 30,
      y: 0,
    });
    expect(snap({ h: 'left', v: 'top', x: 50, y: 70 }, { ...b, minTop: 56 })).toEqual({
      h: 'left',
      v: 'top',
      x: 50,
      y: 56,
    });
  });

  it('keeps its edge offset when the window resizes, and stays on screen', () => {
    const p = { h: 'right', v: 'bottom', x: 16, y: 16 } as const;
    expect(resolve(p, { ...b, W: 800, H: 500 })).toEqual({
      left: 484,
      top: 384,
    });
    expect(resolve({ h: 'right', v: 'bottom', x: 900, y: 16 }, { ...b, W: 400 })).toEqual({ left: 0, top: 484 });
  });

  it('rejects the old top-left format', () => {
    expect(isFloatPos({ x: 10, y: 20 })).toBe(false);
    expect(isFloatPos({ h: 'left', v: 'top', x: 1, y: 2 })).toBe(true);
  });
});
