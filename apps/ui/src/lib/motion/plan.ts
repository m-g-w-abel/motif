import { ID, type Box, type Map2 } from './fit';
import type { Span } from './transition';

/**
 * The planner (docs/ANIMATION.md, Building transitions): given what a
 * stage showed before a change and after it, decides what each part does.
 * Pure, so the rules are tested without a browser; `change.ts` measures
 * and draws.
 */

/** How a part with no counterpart comes in, and (mirrored) goes. */
export type Enter = 'right' | 'left' | 'below' | 'above' | 'rise' | 'fade' | 'grow';

/** A part of a stage as a snapshot saw it: an element with `data-move` or `data-enter`. */
export interface Part {
  /** Its element: the same one on both sides is kept. */
  el: unknown;
  /** `data-move`: the same name on the other side is the same thing. */
  name?: string;
  enter?: Enter;
  /** `data-span`: a name in the stage's span table. */
  span?: string;
  box: Box;
  /** The nearest part on the same side it sits inside. */
  parent?: number;
  /**
   * Not declared: an element between parts (a row holding a part and other
   * buttons). It moves when kept, so what it holds moves with it, and never
   * comes or goes on its own.
   */
  implicit?: boolean;
  /** It clips what it holds (overflow other than visible). */
  clips?: boolean;
}

export type Frame = { map: Map2; opacity?: number };
/** A move: it always runs the whole transition, so everything that moves starts at the start. */
type Moved<K extends string> = Record<K, number> & { frames: [Frame, Frame] };
/** A part coming or going: over its span (`data-span`), so it can come once the movers are nearly there. */
type Faded<K extends string> = Moved<K> & { span: Span };

export interface Plan {
  /** Kept elements that moved: from their old place (less what the part they are in moves) to rest. */
  place: Moved<'after'>[];
  /** A name on another element after: the mover carries it. */
  fly: { before: number; after: number }[];
  enter: Faded<'after'>[];
  /** Played on a copy, since the change took the element away. */
  leave: Faded<'before'>[];
  /** How far things move (px), for a finger to drive it by. */
  distance: number;
}

/** How far `rise` lifts. */
export const RISE_BY = 40;
const LEAST_DISTANCE = 100;

const shift = (x: number, y: number): Map2 => ({ sx: 1, sy: 1, x, y });

/** Where a part with box `b` starts when it comes in as `enter`. */
function away(enter: Enter, b: Box, stage: Box): Frame {
  switch (enter) {
    case 'below':
      return { map: shift(0, stage.top + stage.height - b.top) };
    case 'above':
      return { map: shift(0, stage.top - b.top - b.height) };
    case 'right':
      return { map: shift(stage.left + stage.width - b.left, 0) };
    case 'left':
      return { map: shift(stage.left - b.left - b.width, 0) };
    case 'rise':
      return { map: shift(0, RISE_BY), opacity: 0 };
    case 'fade':
      return { map: ID, opacity: 0 };
    case 'grow': {
      const s = 0.95;
      return { map: { sx: s, sy: s, x: (b.width * (1 - s)) / 2, y: (b.height * (1 - s)) / 2 }, opacity: 0 };
    }
  }
}

const rest = (from: Frame): Frame => (from.opacity === undefined ? { map: ID } : { map: ID, opacity: 1 });

export function plan(before: Part[], after: Part[], stage: Box, spans: Record<string, Span> = {}): Plan {
  const spanOf = (p: Part, kind: 'enter' | 'leave'): Span => (p.span && spans[p.span]) || spans[kind] || [0, 1];
  const kept = after.map((a) => before.findIndex((b) => b.el === a.el));
  const keptBefore = new Set(kept.filter((i) => i >= 0));
  const fly: Plan['fly'] = [];
  const flown = new Set<number>();
  after.forEach((a, i) => {
    if (kept[i] >= 0 || !a.name || a.implicit) return;
    const j = before.findIndex((b, k) => b.name === a.name && !keptBefore.has(k));
    if (j < 0) return;
    fly.push({ before: j, after: i });
    flown.add(j);
  });
  const flownTo = new Set(fly.map((f) => f.after));
  const entering = new Set(after.map((_, i) => i).filter((i) => kept[i] < 0 && !flownTo.has(i) && !after[i].implicit));
  const leaving = new Set(before.map((_, j) => j).filter((j) => !keptBefore.has(j) && !flown.has(j) && !before[j].implicit));
  /** Inside something that comes, goes or flies on `side`: it rides along (or is hidden with it). */
  const carried = (side: Part[], i: number, moving: Set<number>) => {
    for (let p = side[i].parent; p !== undefined; p = side[p].parent) if (moving.has(p)) return true;
    return false;
  };

  /** Whether a kept part moving inside the clipping part `c` (after) lies outside it at either end: cut off on the way. */
  const cut = (i: number, c: number) => {
    const [a, b] = [after[i].box, before[kept[i]].box];
    const [ca, cb] = [after[c].box, kept[c] >= 0 ? before[kept[c]].box : after[c].box];
    const at = (x: Box, y: Box) => ({ left: x.left - y.left, top: x.top - y.top });
    const [ra, rb] = [at(a, ca), at(b, cb)];
    if (Math.abs(ra.left - rb.left) < 0.5 && Math.abs(ra.top - rb.top) < 0.5) return false;
    const inside = (r: { left: number; top: number }, x: Box, y: Box) => r.left >= -1 && r.top >= -1 && r.left + x.width <= y.width + 1 && r.top + x.height <= y.height + 1;
    return !inside(ra, a, ca) || !inside(rb, b, cb);
  };

  // Kept parts, outside in: each moves by its own shift less what the part it is in already shows;
  // one that would be cut off by a box that clips it travels as a copy instead.
  const place: Plan['place'] = [];
  const shown = new Map<number, { x: number; y: number }>();
  after.forEach((a, i) => {
    const outer = a.parent === undefined ? { x: 0, y: 0 } : (shown.get(a.parent) ?? { x: 0, y: 0 });
    // New or gone parts don't move on their own: what they hold moves with what they are in.
    if (kept[i] < 0 || carried(after, i, flownTo)) return void shown.set(i, outer);
    const b = before[kept[i]];
    const x = b.box.left - a.box.left;
    const y = b.box.top - a.box.top;
    if (Math.abs(x - outer.x) < 0.5 && Math.abs(y - outer.y) < 0.5) return void shown.set(i, outer);
    shown.set(i, { x, y });
    // Declared parts only: a container between parts (a whole list) moving in its scroller is clipped by it, as it is at rest.
    for (let c = a.parent; c !== undefined && !a.implicit; c = after[c].parent)
      if (after[c].clips && cut(i, c)) {
        fly.push({ before: kept[i], after: i });
        flownTo.add(i);
        return;
      }
    place.push({ after: i, frames: [{ map: shift(x - outer.x, y - outer.y) }, { map: ID }] });
  });

  const enter: Plan['enter'] = [];
  for (const i of entering) {
    if (carried(after, i, new Set([...entering, ...flownTo]))) continue;
    const from = away(after[i].enter ?? 'fade', after[i].box, stage);
    enter.push({ after: i, frames: [from, rest(from)], span: spanOf(after[i], 'enter') });
  }
  const leave: Plan['leave'] = [];
  for (const j of leaving) {
    if (carried(before, j, new Set([...leaving, ...flown]))) continue;
    const to = away(before[j].enter ?? 'fade', before[j].box, stage);
    leave.push({ before: j, frames: [rest(to), to], span: spanOf(before[j], 'leave') });
  }

  const far = [
    ...place.map((m) => m.frames[0].map.y),
    ...enter.map((m) => m.frames[0].map.y),
    ...leave.map((m) => m.frames[1].map.y),
    ...fly.map((f) => after[f.after].box.top - before[f.before].box.top),
  ].map(Math.abs);
  return { place, fly, enter, leave, distance: Math.max(LEAST_DISTANCE, ...far) };
}
