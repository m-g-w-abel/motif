/**
 * Geometry for moving things (docs/ANIMATION.md): boxes and the maps
 * between them. Pure, so where things land is tested without a browser.
 */

export type Box = { left: number; top: number; width: number; height: number };

/** A move and scale, origin top left: (x, y) ↦ (sx·x + x, sy·y + y). */
export type Map2 = { sx: number; sy: number; x: number; y: number };

export const ID: Map2 = { sx: 1, sy: 1, x: 0, y: 0 };

/** `a` after `b`. */
export const compose = (a: Map2, b: Map2): Map2 => ({ sx: a.sx * b.sx, sy: a.sy * b.sy, x: a.sx * b.x + a.x, y: a.sy * b.y + a.y });

export const invert = (m: Map2): Map2 => ({ sx: 1 / m.sx, sy: 1 / m.sy, x: -m.x / m.sx, y: -m.y / m.sy });

export const lerp = (a: Map2, b: Map2, t: number): Map2 => ({
  sx: a.sx + (b.sx - a.sx) * t,
  sy: a.sy + (b.sy - a.sy) * t,
  x: a.x + (b.x - a.x) * t,
  y: a.y + (b.y - a.y) * t,
});

export const apply = (m: Map2, b: Box): Box => ({ left: m.sx * b.left + m.x, top: m.sy * b.top + m.y, width: m.sx * b.width, height: m.sy * b.height });

/** The map that puts box `a` on box `b`. */
export const onto = (a: Box, b: Box): Map2 => {
  const sx = b.width / a.width || 1;
  const sy = b.height / a.height || 1;
  return { sx, sy, x: b.left - sx * a.left, y: b.top - sy * a.top };
};

/** Box `b` in the coordinates of a drawing whose top left is at `at`. */
const local = (b: Box, at: Box): Box => ({ left: b.left - at.left, top: b.top - at.top, width: b.width, height: b.height });

const shift = (x: number, y: number): Map2 => ({ sx: 1, sy: 1, x, y });

/** As a CSS transform (with `transform-origin: 0 0`). */
export const css = (m: Map2) => `translate(${m.x}px, ${m.y}px) scale(${m.sx}, ${m.sy})`;

/**
 * A drawing that travels from box `from` to box `to`: a copy of the old one
 * placed at `from` (`'from'`), or of the new one placed at `to`. Its two
 * transforms, in its own coordinates.
 */
export function boxFlight(from: Box, to: Box, copy: 'from' | 'to') {
  const there = copy === 'from' ? onto(local(from, from), local(to, from)) : onto(local(to, to), local(from, to));
  return { frames: copy === 'from' ? [ID, there] : [there, ID] };
}

/**
 * Words that move from `from` to `to` (element boxes; `fromGlyph` and
 * `toGlyph` the first letter's), as `steps + 1` keyframes of three maps:
 * - `clip`: a box placed at `to` that clips the words, from the old element's
 *   box to the new one's, so a long title is cut where each end cuts it;
 * - `dest`: the new drawing (placed at the clip's top left), scaled evenly
 *   from the old words' size and place to its own, counter to the clip's
 *   stretch (sampled, since that isn't linear);
 * - `source`: the old drawing (placed at `from`'s offset from `to`), locked
 *   onto the new words, to fade out over them where the two look different.
 * With `ink` (each drawing's words' width, when neither is cut off), the two
 * also stretch a little across, so that in another weight their words span
 * the same width all the way: the old at its own width at the start, the new
 * at its own at the end.
 */
export function textFlight({ from, fromGlyph, to, toGlyph, steps, ink }: { from: Box; fromGlyph: Box; to: Box; toGlyph: Box; steps: number; ink?: { from: number; to: number } }) {
  const s = fromGlyph.height / toGlyph.height;
  const a = local(fromGlyph, to);
  const b = local(toGlyph, to);
  const words0: Map2 = { sx: s, sy: s, x: a.left - s * b.left, y: a.top + a.height / 2 - s * (b.top + b.height / 2) };
  const clip0 = onto(local(to, to), local(from, to));
  const at = shift(from.left - to.left, from.top - to.top);
  // The old drawing in the new one's coordinates.
  const old = compose(invert(words0), at);
  const clip: Map2[] = [];
  const dest: Map2[] = [];
  const source: Map2[] = [];
  // How much wider the new words are than the old, at the same size; each drawing stretches across about its first letter.
  const k = ink && ink.from > 0 ? (ink.to * s) / ink.from : 1;
  const across = (w: number, x: number): Map2 => ({ sx: w, sy: 1, x: x * (1 - w), y: 0 });
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    const o = lerp(clip0, ID, t);
    const words = lerp(words0, ID, t);
    const w = 1 + (k - 1) * t;
    clip.push(o);
    dest.push(compose(invert(o), compose(words, across(w / k, b.left))));
    source.push(compose(invert(at), compose(invert(o), compose(words, compose(old, across(w, fromGlyph.left - from.left))))));
  }
  return { clip, dest, source };
}
