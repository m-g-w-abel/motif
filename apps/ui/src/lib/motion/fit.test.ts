import { describe, expect, it } from 'vitest';
import { apply, boxFlight, compose, ID, invert, textFlight, type Box, type Map2 } from './fit';

const box = (left: number, top: number, width: number, height: number): Box => ({ left, top, width, height });
const near = (a: Box, b: Box) => {
  for (const k of ['left', 'top', 'width', 'height'] as const) expect(a[k]).toBeCloseTo(b[k], 3);
};
const near2 = (a: Map2, b: Map2) => {
  for (const k of ['sx', 'sy', 'x', 'y'] as const) expect(a[k]).toBeCloseTo(b[k], 6);
};
/** Where a box inside a drawing (its local coordinates) is on screen, when the drawing sits at `at` inside a clip at `clip` moved by `o`. */
const onScreen = (clip: Box, o: Map2, at: { x: number; y: number }, local: Map2, inner: Box) => {
  const shifted = compose({ sx: 1, sy: 1, x: clip.left, y: clip.top }, compose(o, compose({ sx: 1, sy: 1, x: at.x, y: at.y }, local)));
  return apply(shifted, inner);
};

describe('maps', () => {
  it('compose and invert', () => {
    const m = { sx: 2, sy: 3, x: 5, y: -4 };
    near2(compose(m, invert(m)), ID);
    near2(compose(invert(m), m), ID);
  });
});

describe('boxFlight', () => {
  it('lands exactly on the new box, whichever drawing travels', () => {
    const from = box(10, 20, 50, 50);
    const to = box(100, 200, 300, 300);
    /** The copy's box on screen: placed at `at`, moved by `m` in its own coordinates. */
    const drawn = (m: Map2, at: Box) => apply(compose({ sx: 1, sy: 1, x: at.left, y: at.top }, m), box(0, 0, at.width, at.height));
    const old = boxFlight(from, to, 'from');
    near(drawn(old.frames[0], from), from);
    near(drawn(old.frames[1], from), to);
    const own = boxFlight(from, to, 'to');
    near(drawn(own.frames[0], to), from);
    near(drawn(own.frames[1], to), to);
  });
});

describe('textFlight', () => {
  // A bar's small title, cut off at 120 px, flies to the sheet's big one, 300 px wide.
  const from = box(60, 700, 120, 18);
  const fromGlyph = box(60, 702, 9, 14);
  const to = box(20, 300, 300, 40);
  const toGlyph = box(20, 304, 20, 32);
  const f = textFlight({ from, fromGlyph, to, toGlyph, steps: 8 });

  it('starts with its words over the old words, clipped to the old box', () => {
    near(onScreen(to, f.clip[0], { x: 0, y: 0 }, f.dest[0], { left: toGlyph.left - to.left, top: toGlyph.top - to.top, width: toGlyph.width, height: toGlyph.height }), {
      left: fromGlyph.left,
      top: fromGlyph.top,
      width: (toGlyph.width * fromGlyph.height) / toGlyph.height,
      height: fromGlyph.height,
    });
    near(apply(compose({ sx: 1, sy: 1, x: to.left, y: to.top }, f.clip[0]), box(0, 0, to.width, to.height)), from);
  });

  it('lands exactly as the new element sits, unclipped and unscaled', () => {
    near2(f.clip.at(-1)!, ID);
    near2(f.dest.at(-1)!, ID);
    expect(f.clip).toHaveLength(9);
  });

  it('never squashes the words: on the way, they scale evenly however the clip stretches', () => {
    for (let k = 0; k < f.clip.length; k++) {
      const whole = compose(f.clip[k], f.dest[k]);
      expect(whole.sx).toBeCloseTo(whole.sy, 6);
    }
  });

  it('carries the old drawing with the words, from exactly where it was', () => {
    const at = { x: from.left - to.left, y: from.top - to.top };
    near(onScreen(to, f.clip[0], at, f.source[0], box(0, 0, from.width, from.height)), from);
    // Locked to the new words all the way: the old glyph sits on the new glyph at every step.
    for (let k = 0; k < f.clip.length; k++) {
      const local = (g: Box, b: Box) => box(g.left - b.left, g.top - b.top, g.width, g.height);
      const a = onScreen(to, f.clip[k], at, f.source[k], local(fromGlyph, from));
      const b = onScreen(to, f.clip[k], { x: 0, y: 0 }, f.dest[k], local(toGlyph, to));
      expect(a.left).toBeCloseTo(b.left, 3);
      expect(a.top + a.height / 2).toBeCloseTo(b.top + b.height / 2, 3);
      expect(a.height).toBeCloseTo(b.height, 3);
    }
  });
});

describe('textFlight between two weights', () => {
  // A regular title 100 px wide becomes a bold one 2× the size and 230 px wide (115 at the old size).
  const from = box(10, 10, 120, 20);
  const fromGlyph = box(10, 12, 8, 16);
  const to = box(50, 200, 300, 40);
  const toGlyph = box(50, 204, 18, 32);
  const f = textFlight({ from, fromGlyph, to, toGlyph, steps: 8, ink: { from: 100, to: 230 } });
  const at = { x: from.left - to.left, y: from.top - to.top };
  /** On screen, the span of a drawing's words (`left` in its own coordinates, `width` wide). */
  const span = (k: number, which: 'dest' | 'source') => {
    const [place, left, width] = which === 'dest' ? [{ x: 0, y: 0 }, toGlyph.left - to.left, 230] : [at, fromGlyph.left - from.left, 100];
    return onScreen(to, f.clip[k], place, f[which][k], box(left, 0, width, 1));
  };

  it('keeps both drawings over the same words all the way, so they never ghost', () => {
    for (let k = 0; k < f.clip.length; k++) {
      const [a, b] = [span(k, 'source'), span(k, 'dest')];
      expect(a.left).toBeCloseTo(b.left, 3);
      expect(a.width).toBeCloseTo(b.width, 3);
    }
  });

  it('still starts exactly as the old drawing and lands exactly as the new', () => {
    near(onScreen(to, f.clip[0], at, f.source[0], box(0, 0, from.width, from.height)), from);
    near2(compose(f.clip.at(-1)!, f.dest.at(-1)!), ID);
  });
});
