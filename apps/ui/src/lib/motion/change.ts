import { css, type Box } from './fit';
import { behind, cloneOf, copyOf, fly, motionLayer, rootZoom, see, type Flight, type Seen } from './mover';
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
  /** Named stretches of progress (`data-span`) for parts that come or go, and `enter` and `leave` for those that name none. Moves always run the whole way. */
  spans?: Record<string, Span>;
  /** A positioned element in the stage that copies of leaving parts go in (they keep its rules); else above everything. */
  under?: HTMLElement;
  /** A layer behind the stage's content, above its backdrop, where copies travel to or from parts that lie behind it (`behind`). */
  back?: HTMLElement;
  /** The transition ended, at the new state (`landed`) or back at the old, after cleaning up. */
  ended?: (landed: boolean) => void;
  /** Whether parts that went play their way out; without, they are gone at once (a row swiped out already). */
  leaves?: boolean;
}

export type Change = Transition & { distance: number };

/** Drawn: laid out and not hidden. */
const shows = (el: HTMLElement) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';

type Snapshot = { parts: Part[]; els: HTMLElement[]; seen: Seen[]; clones: (HTMLElement | null)[]; drawn: Set<Element> };

/** The stage's parts as they are now, in CSS pixels, each with a copy to stand in for it if the change takes it away. */
function snapshot(root: HTMLElement, copies: boolean): Snapshot {
  const z = rootZoom();
  const declared = [...root.querySelectorAll<HTMLElement>('[data-move], [data-enter]')].filter(shows);
  // The elements between parts and up to the stage (implicit parts): when one moves, what it holds moves with it.
  const all = new Set(declared);
  for (const el of declared)
    for (let up = el.parentElement; up && up !== root && !all.has(up); up = up.parentElement) if (!up.matches('[data-move], [data-enter]') && shows(up)) all.add(up);
  const els = [...all].sort((a, b) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
  const seen = els.map((el) => see(el));
  const parts = els.map((el, i): Part => {
    const b = seen[i].box;
    let parent: number | undefined;
    for (let j = i - 1; j >= 0 && parent === undefined; j--) if (els[j].contains(el)) parent = j;
    const style = getComputedStyle(el);
    return {
      el,
      name: el.dataset.move,
      enter: el.dataset.enter as Enter | undefined,
      span: el.dataset.span,
      box: { left: b.left / z, top: b.top / z, width: b.width / z, height: b.height / z },
      parent,
      implicit: !el.matches('[data-move], [data-enter]'),
      clips: style.overflowX !== 'visible' || style.overflowY !== 'visible',
    };
  });
  // Only declared parts ever leave, so only they need a copy.
  const clones = els.map((el, i) => (copies && !parts[i].implicit ? cloneOf(el) : null));
  // What flies from a part the change takes away is copied from these, made while it was drawn.
  clones.forEach((c, i) => c && (seen[i].copy = c));
  for (const s of seen) s.behind = behind(s.el, root);
  // Everything drawn, to find what a change shows that nothing declared (`draw`).
  const drawn = new Set([...root.querySelectorAll('*')].filter((el) => el.getClientRects().length && getComputedStyle(el).visibility !== 'hidden'));
  return { parts, els, seen, clones, drawn };
}

const frame = ({ map, opacity }: Frame): Keyframe => ({ transform: css(map), ...(opacity !== undefined && { opacity }) });

export function change(root: HTMLElement, apply: () => void, options: Options = {}): Change {
  const then = measure(root, options);
  try {
    apply();
  } catch (e) {
    // Not drawn: drop the measured start, or the stage's next change would start from it.
    waiting.delete(root);
    throw e;
  }
  return then();
}

/**
 * `change` in two halves, for a state that changes outside a callback (an
 * engine's update): measures now, and returns what draws the transition
 * once the DOM has changed.
 */
export function measure(root: HTMLElement, options: Options = {}): () => Change {
  // A second change before the first is drawn (two updates in one tick) starts from the same place: the
  // layout now is already the first's end, and starting there would jump.
  const before = waiting.get(root) ?? snapshot(options.from ?? root, options.leaves !== false);
  waiting.set(root, before);
  return () => {
    waiting.delete(root);
    return draw(root, before, options);
  };
}

/** Stages measured and not yet drawn: what they showed before. */
const waiting = new Map<HTMLElement, Snapshot>();

/** The changes under way, by the stage they run in. */
const running = new Map<Change, HTMLElement>();

function draw(root: HTMLElement, before: Snapshot, options: Options): Change {
  const { spans, under, back, ended, leaves = true } = options;
  // One change at a time in a stage: one still running in it (rows still sliding when the next update
  // comes) ends, and this one starts from where that left things drawn, as `before` measured them.
  for (const [c, r] of running) if (root.contains(r)) c.end();
  const after = snapshot(root, false);
  const z = rootZoom();
  const r = root.getBoundingClientRect();
  const stage: Box = { left: r.left / z, top: r.top / z, width: r.width / z, height: r.height / z };
  const p = plan(before.parts, after.parts, stage, spans);

  const tracks: Track[] = [];
  /** Inline transform origins to put back (keyframes scale about the top left). */
  const origins = new Map<HTMLElement, string>();
  const own = (el: HTMLElement, frames: [Frame, Frame], span?: Span) => {
    // Its own shift (a row a reorder moved aside) stays under the move, which would otherwise replace it.
    // shortcut: only a plain shift is kept (one with a scale would scale about the new origin); none has one yet.
    const m = new DOMMatrix(getComputedStyle(el).transform === 'none' ? undefined : getComputedStyle(el).transform);
    const base = m.isIdentity || !(m.is2D && m.a === 1 && m.d === 1 && !m.b && !m.c) ? '' : ` translate(${m.e}px, ${m.f}px)`;
    if (!origins.has(el)) origins.set(el, el.style.transformOrigin);
    el.style.transformOrigin = '0 0';
    tracks.push({ el, frames: frames.map((f) => ({ ...frame(f), transform: css(f.map) + base })), span });
  };
  for (const m of p.place) own(after.els[m.after], m.frames);
  // A box that clips and changes size can't have its clip moved by a transform (its old clip would be its new
  // size in its old place, cutting off what it held): it stops clipping while the change runs.
  const unclipped = new Map<HTMLElement, string>();
  after.parts.forEach((a, i) => {
    const j = before.parts.findIndex((b) => b.el === a.el);
    if (!a.clips || j < 0) return;
    const was = before.parts[j].box;
    if (Math.abs(was.width - a.box.width) < 1 && Math.abs(was.height - a.box.height) < 1) return;
    const el = after.els[i];
    unclipped.set(el, el.style.overflow);
    el.style.overflow = 'visible';
  });
  for (const m of p.enter) own(after.els[m.after], m.frames, m.span);

  // Shown by the change but declared nowhere (a part's insides it brings): faded in, never popping in.
  // What comes in or flies has its own way in; what it holds comes with it.
  const coming = [...p.enter.map((m) => after.els[m.after]), ...p.fly.map((f) => after.els[f.after])];
  const before_ = options.from ? null : before.drawn;
  for (const el of before_ ? after.drawn : []) {
    if (before_!.has(el) || (el.parentElement && after.drawn.has(el.parentElement) && !before_!.has(el.parentElement)) || !(el instanceof HTMLElement || el instanceof SVGElement)) continue;
    if (coming.some((c) => c.contains(el)) || el.closest('[data-move], [data-enter]') === el) continue;
    tracks.push({ el: el as HTMLElement, frames: [{ opacity: 0 }, { opacity: 1 }], span: spans?.enter });
  }

  const flights: Flight[] = [];
  const flown = new Set<string>();
  for (const f of p.fly) {
    const flight = fly(before.seen[f.before], after.els[f.after], after.seen[f.after].box, { root, back });
    tracks.push(...flight.tracks);
    flights.push(flight);
    flown.add(before.parts[f.before].name!);
  }

  // Copies of what went, standing where it was; what flies out of them is left out (its copy travels).
  const copies: HTMLElement[] = [];
  const at = under?.getBoundingClientRect();
  for (const m of leaves ? p.leave : []) {
    const s = before.seen[m.before];
    const clone = before.clones[m.before]!.cloneNode(true) as HTMLElement;
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
    for (const [el, o] of unclipped) el.style.overflow = o;
    running.delete(t);
    ended?.(landed);
  }) as Change;
  t.distance = p.distance;
  running.set(t, root);
  return t;
}
