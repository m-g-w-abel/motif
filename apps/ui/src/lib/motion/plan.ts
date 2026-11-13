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
}

export type Frame = { map: Map2; opacity?: number };
type Moved<K extends string> = Record<K, number> & { frames: [Frame, Frame]; span: Span };

export interface Plan {
  /** Kept elements that moved: from their old place (less what the part they are in moves) to rest. */
  place: Moved<'after'>[];
  /** A name on another element after: the mover carries it. */
  fly: { before: number; after: number; span: Span }[];
  enter: Moved<'after'>[];
  /** Played on a copy, since the change took the element away. */
  leave: Moved<'before'>[];
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
  const spanOf = (p: Part, kind: 'move' | 'enter' | 'leave'): Span => (p.span && spans[p.span]) || spans[kind] || [0, 1];
  const kept = after.map((a) => before.findIndex((b) => b.el === a.el));
  const keptBefore = new Set(kept.filter((i) => i >= 0));
  const fly: Plan['fly'] = [];
  const flown = new Set<number>();
  after.forEach((a, i) => {
    if (kept[i] >= 0 || !a.name) return;
    const j = before.findIndex((b, k) => b.name === a.name && !keptBefore.has(k));
    if (j < 0) return;
    fly.push({ before: j, after: i, span: spanOf(a.span ? a : before[j], 'move') });
    flown.add(j);
  });
  const flownTo = new Set(fly.map((f) => f.after));
  const entering = new Set(after.map((_, i) => i).filter((i) => kept[i] < 0 && !flownTo.has(i)));
  const leaving = new Set(before.map((_, j) => j).filter((j) => !keptBefore.has(j) && !flown.has(j)));
  /** Inside something that comes, goes or flies on `side`: it rides along (or is hidden with it). */
  const carried = (side: Part[], i: number, moving: Set<number>) => {
    for (let p = side[i].parent; p !== undefined; p = side[p].parent) if (moving.has(p)) return true;
    return false;
  };

  // Kept parts, outside in: each moves by its own shift less what the part it is in already shows.
  const place: Plan['place'] = [];
  const shown = new Map<number, { x: number; y: number }>();
  after.forEach((a, i) => {
    if (kept[i] < 0) return;
    const b = before[kept[i]];
    const x = b.box.left - a.box.left;
    const y = b.box.top - a.box.top;
    const outer = a.parent === undefined ? { x: 0, y: 0 } : (shown.get(a.parent) ?? { x: 0, y: 0 });
    if (Math.abs(x - outer.x) < 0.5 && Math.abs(y - outer.y) < 0.5) return void shown.set(i, outer);
    shown.set(i, { x, y });
    place.push({ after: i, frames: [{ map: shift(x - outer.x, y - outer.y) }, { map: ID }], span: spanOf(a, 'move') });
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
