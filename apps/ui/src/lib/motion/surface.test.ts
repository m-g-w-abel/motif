import { describe, expect, it } from 'vitest';
import type { Box } from './fit';
import { morphAt } from './surface';

const box = (left: number, top: number, width: number, height: number): Box => ({ left, top, width, height });
/** The part of `to` drawn on screen: moved by the morph's translate, cut by its inset. */
const drawn = (to: Box, m: ReturnType<typeof morphAt>) =>
  box(to.left + m.x + m.inset[3], to.top + m.y + m.inset[0], to.width - m.inset[1] - m.inset[3], to.height - m.inset[0] - m.inset[2]);
const near = (a: Box, b: Box) => {
  for (const k of ['left', 'top', 'width', 'height'] as const) expect(a[k]).toBeCloseTo(b[k], 6);
};

describe('morphAt', () => {
  const button = box(330, 60, 40, 36);
  const modal = box(16, 300, 352, 240);

  it('starts on the button and lands on the modal, corners included', () => {
    near(drawn(modal, morphAt(button, 3, modal, 4, 0)), button);
    near(drawn(modal, morphAt(button, 3, modal, 4, 1)), modal);
    expect(morphAt(button, 3, modal, 4, 0).radius).toBe(3);
    expect(morphAt(button, 3, modal, 4, 1)).toEqual({ x: 0, y: 0, inset: [0, 0, 0, 0], radius: 4 });
  });

  it('grows in between, keeping what it holds unscaled and centred', () => {
    const half = drawn(modal, morphAt(button, 3, modal, 4, 0.5));
    near(half, box(173, 180, 196, 138));
    expect(morphAt(button, 3, modal, 4, 0.5).radius).toBe(3.5);
  });
});
