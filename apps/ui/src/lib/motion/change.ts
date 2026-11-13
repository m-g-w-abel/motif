import { css, type Box } from './fit';
import { copyOf, fly, motionLayer, rootZoom, see, type Flight, type Seen } from './mover';
import { plan, type Enter, type Frame, type Part } from './plan';
import { Transition, type Span, type Track } from './transition';

/**
 * Measure, change, measure (docs/ANIMATION.md): snapshots the parts of a
 * stage that declare a role (`data-move`, `data-enter`, `data-span`),
 * changes the state, snapshots again and draws the plan between the two as
 * one transition, ready to be played (a tap) or driven (a finger).
 */

export interface Options {
  /** Where the "before" parts are, when not in `root` (the player bar, before the sheet). */
  from?: HTMLElement;
  /** Named stretches of progress (`data-span`), and `move`, `enter` and `leave` for parts that name none. */
  spans?: Record<string, Span>;
  /** A positioned element in the stage that copies of leaving parts go in (they keep its rules); else above everything. */
  under?: HTMLElement;
  /** The transition ended, at the new state (`landed`) or back at the old, after cleaning up. */
  ended?: (landed: boolean) => void;
}

export type Change = Transition & { distance: number };

/** Drawn: laid out and not hidden. */
const shows = (el: HTMLElement) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';

type Snapshot = { parts: Part[]; els: HTMLElement[]; seen: Seen[]; clones: HTMLElement[] };

/** The stage's parts as they are now, in CSS pixels, each with a copy to stand in for it if the change takes it away. */
function snapshot(root: HTMLElement, copies: boolean): Snapshot {
  const z = rootZoom();
  const els = [...root.querySelectorAll<HTMLElement>('[data-move], [data-enter]')].filter(shows);
  const seen = els.map((el) => see(el));
  const parts = els.map((el, i): Part => {
    const b = seen[i].box;
    let parent: number | undefined;
    for (let j = i - 1; j >= 0 && parent === undefined; j--) if (els[j].contains(el)) parent = j;
    return {
      el,
      name: el.dataset.move,
      enter: el.dataset.enter as Enter | undefined,
      span: el.dataset.span,
      box: { left: b.left / z, top: b.top / z, width: b.width / z, height: b.height / z },
      parent,
    };
  });
  return { parts, els, seen, clones: copies ? els.map((el) => el.cloneNode(true) as HTMLElement) : [] };
}

const frame = ({ map, opacity }: Frame): Keyframe => ({ transform: css(map), ...(opacity !== undefined && { opacity }) });

export function change(root: HTMLElement, apply: () => void, { from = root, spans, under, ended }: Options = {}): Change {
  const before = snapshot(from, true);
  apply();
  const after = snapshot(root, false);
  const z = rootZoom();
  const r = root.getBoundingClientRect();
  const stage: Box = { left: r.left / z, top: r.top / z, width: r.width / z, height: r.height / z };
  const p = plan(before.parts, after.parts, stage, spans);

  const tracks: Track[] = [];
  /** Inline transform origins to put back (keyframes scale about the top left). */
  const origins = new Map<HTMLElement, string>();
  const own = (el: HTMLElement, frames: [Frame, Frame], span: Span) => {
    if (!origins.has(el)) origins.set(el, el.style.transformOrigin);
    el.style.transformOrigin = '0 0';
    tracks.push({ el, frames: frames.map(frame), span });
  };
  for (const m of p.place) own(after.els[m.after], m.frames, m.span);
  for (const m of p.enter) own(after.els[m.after], m.frames, m.span);

  const flights: Flight[] = [];
  const flown = new Set<string>();
  for (const f of p.fly) {
    const flight = fly(before.seen[f.before], after.els[f.after], after.seen[f.after].box);
    for (const t of flight.tracks) tracks.push({ ...t, span: f.span });
    flights.push(flight);
    flown.add(before.parts[f.before].name!);
  }

  // Copies of what went, standing where it was; what flies out of them is left out (its copy travels).
  const copies: HTMLElement[] = [];
  const at = under?.getBoundingClientRect();
  for (const m of p.leave) {
    const s = before.seen[m.before];
    const clone = before.clones[m.before];
    for (const n of clone.querySelectorAll<HTMLElement>('[data-move]')) if (flown.has(n.dataset.move!)) n.style.visibility = 'hidden';
    const b = before.parts[m.before].box;
    const c = copyOf(s, at ? { ...b, left: b.left - at.left / z, top: b.top - at.top / z } : b, !at, clone);
    c.inert = true;
    if (under) under.append(c);
    else motionLayer().append(c);
    copies.push(c);
    tracks.push({ el: c, frames: m.frames.map(frame), span: m.span });
  }

  const t = new Transition(tracks, () => {
    const landed = t.heading === 1;
    for (const f of flights) f.done(landed);
    for (const c of copies) c.remove();
    for (const [el, o] of origins) el.style.transformOrigin = o;
    ended?.(landed);
  }) as Change;
  t.distance = p.distance;
  return t;
}
