import { css, ID, type Box } from './fit';
import { behind, cloneOf, copyOf, fly, full, motionLayer, rescroll, rootZoom, see, type Flight, type Seen } from './mover';
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
  /** Left as it is drawn, with what it holds: a row a finger drags while the others make way. */
  still?: HTMLElement;
}

/** `distance`: how far things move on screen (px), for a finger to drive it by. */
export type Change = Transition & { distance: number };

/** Drawn: laid out and not hidden. */
const shows = (el: HTMLElement) => el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';

/** Where a drawn element was (CSS pixels), and where it sits in its nearest part's copy (that part, and the path of child indices). */
type Place = { box: Box; part?: number; path: number[] };
type Snapshot = { parts: Part[]; els: HTMLElement[]; seen: Seen[]; clones: (HTMLElement | null)[]; drawn: Map<Element, Place> };

/** The stage's parts as they are now, in CSS pixels, each with a copy to stand in for it if the change takes it away. */
function snapshot(root: HTMLElement, copies: boolean): Snapshot {
  const z = rootZoom();
  const declared = [...root.querySelectorAll<HTMLElement>('[data-move], [data-enter]')].filter(shows);
  // The elements between parts and up to the stage (implicit parts): when one moves, what it holds moves with it.
  const all = new Set(declared);
  for (const el of declared)
    for (let up = el.parentElement; up && up !== root && !all.has(up); up = up.parentElement) if (!up.matches('[data-move], [data-enter]') && shows(up)) all.add(up);
  const els = [...all].sort((a, b) => (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
  // Containers between parts (implicit) only ever move in place: their box is all that is needed.
  const seen = els.map((el): Seen => {
    if (el.matches('[data-move], [data-enter]')) return see(el);
    const box = el.getBoundingClientRect();
    return { el, box, view: box, glyph: box, ink: 0, text: false, look: {}, cut: false, res: 0 };
  });
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
  // A part inside another part is copied as that part of its copy, not afresh (when the outer copy kept its insides' look).
  const clones: (HTMLElement | null)[] = [];
  els.forEach((el, i) => {
    clones[i] = null;
    if (!copies || parts[i].implicit) return;
    let outer = parts[i].parent;
    while (outer !== undefined && parts[outer].implicit) outer = parts[outer].parent;
    const from = outer === undefined ? null : clones[outer];
    clones[i] = (from && full(from) && within(els[outer!], el, from)) || cloneOf(el);
  });
  // What flies from a part the change takes away is copied from these, made while it was drawn.
  clones.forEach((c, i) => c && (seen[i].copy = c));
  for (const s of seen) s.behind = behind(s.el, root);
  // Everything drawn, to find what a change shows or takes away that nothing declared (`draw`): where each
  // was, and where it sits in its part's copy, to stand in for it as it fades out.
  const index = new Map(els.map((el, i) => [el, i]));
  const drawn = new Map<Element, Place>();
  for (const el of root.querySelectorAll('*')) {
    if (!el.getClientRects().length || getComputedStyle(el).visibility === 'hidden') continue;
    const path: number[] = [];
    let e: Element = el;
    while (e !== root && !(index.has(e as HTMLElement) && !parts[index.get(e as HTMLElement)!].implicit)) {
      path.unshift([...e.parentElement!.children].indexOf(e));
      e = e.parentElement!;
    }
    const b = el.getBoundingClientRect();
    drawn.set(el, { box: { left: b.left / z, top: b.top / z, width: b.width / z, height: b.height / z }, part: e === root ? undefined : index.get(e as HTMLElement), path });
  }
  return { parts, els, seen, clones, drawn };
}

/** The node in `copy` (a deep copy of `outer`) that stands where `el` stands in `outer`. */
function within(outer: Element, el: Element, copy: Element): HTMLElement | null {
  const path: number[] = [];
  for (let e: Element = el; e !== outer; e = e.parentElement!) path.unshift([...e.parentElement!.children].indexOf(e));
  let at: Element | undefined = copy;
  for (const k of path) at = at?.children[k];
  return (at as HTMLElement | undefined) ?? null;
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
  const { spans, under, back, ended, leaves = true, still } = options;
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
  const animate = (el: HTMLElement, frames: [Frame, Frame], span?: Span) => {
    // Its own shift (a row a reorder moved aside) stays under the move, which would otherwise replace it.
    // shortcut: only a plain shift is kept (one with a scale would scale about the new origin); none has one yet.
    const m = new DOMMatrix(getComputedStyle(el).transform === 'none' ? undefined : getComputedStyle(el).transform);
    const base = m.isIdentity || !(m.is2D && m.a === 1 && m.d === 1 && !m.b && !m.c) ? '' : ` translate(${m.e}px, ${m.f}px)`;
    if (!origins.has(el)) origins.set(el, el.style.transformOrigin);
    el.style.transformOrigin = '0 0';
    tracks.push({ el, frames: frames.map((f) => ({ ...frame(f), transform: css(f.map) + base })), span });
  };
  for (const m of p.place) animate(after.els[m.after], m.frames);
  // A box that clips and changes size can't have its clip moved by a transform (its old clip would be its new
  // size in its old place, cutting off what it held, or showing what it hid): its clip goes from its old size
  // to its new one instead, as a clip path in its own coordinates (which its move carries), and its own
  // clipping returns at the end. The one property besides transform and opacity that a change animates; only
  // here, for the 260 ms of a change.
  const unclipped = new Map<HTMLElement, { overflow: string; x: number; y: number }>();
  const rect = (w: number, h: number) => `polygon(0px 0px, ${w}px 0px, ${w}px ${h}px, 0px ${h}px)`;
  after.parts.forEach((a, i) => {
    const j = before.parts.findIndex((b) => b.el === a.el);
    if (!a.clips || j < 0) return;
    const was = before.parts[j].box;
    if (Math.abs(was.width - a.box.width) < 1 && Math.abs(was.height - a.box.height) < 1) return;
    const el = after.els[i];
    // Unclipped, a scrolled box is no longer a scroller and loses its position: what it holds is held where it was
    // scrolled to (`translate`, apart from the transforms a change animates) until its scrolling returns.
    const [x, y] = [el.scrollLeft, el.scrollTop];
    unclipped.set(el, { overflow: el.style.overflow, x, y });
    el.style.overflow = 'visible';
    if (x || y) for (const c of el.children) (c as HTMLElement).style.translate = `${-x}px ${-y}px`;
    tracks.push({ el, frames: [{ clipPath: rect(was.width, was.height) }, { clipPath: rect(a.box.width, a.box.height) }] });
  });
  for (const m of p.enter) animate(after.els[m.after], m.frames, m.span);

  // Every other drawn element the change keeps moves from where it was too, by its own share (what the nearest
  // kept element it is in doesn't already show): the skip buttons of a row the change made narrower, re-spaced
  // in it, slide to their places instead of jumping.
  const isPart = new Set<Element>(after.els);
  const shiftOf = (el: Element) => {
    const [b, a] = [before.drawn.get(el)!.box, after.drawn.get(el)!.box];
    return { x: b.left - a.left, y: b.top - a.top };
  };
  const arriving = [...p.enter.map((m) => after.els[m.after]), ...p.fly.map((f) => after.els[f.after])];
  /** Words the change wraps differently (a row that got wider): a move can't get them there, so they crossfade (below). */
  const rewrapped: Element[] = [];
  for (const el of options.from ? [] : after.drawn.keys()) {
    if (isPart.has(el) || !before.drawn.has(el) || !(el instanceof HTMLElement || el instanceof SVGElement) || arriving.some((c) => c.contains(el))) continue;
    const [was, now] = [before.drawn.get(el)!.box, after.drawn.get(el)!.box];
    if ((Math.abs(was.width - now.width) > 1 || Math.abs(was.height - now.height) > 1) && [...el.childNodes].some((c) => c.nodeType === Node.TEXT_NODE && c.textContent!.trim())) rewrapped.push(el);
    let up = el.parentElement;
    while (up && up !== root && !(before.drawn.has(up) && after.drawn.has(up))) up = up.parentElement;
    const own = shiftOf(el);
    const outer = up && up !== root ? shiftOf(up) : { x: 0, y: 0 };
    if (Math.abs(own.x - outer.x) < 0.5 && Math.abs(own.y - outer.y) < 0.5) continue;
    animate(el as HTMLElement, [{ map: { sx: 1, sy: 1, x: own.x - outer.x, y: own.y - outer.y } }, { map: ID }]);
  }

  // Shown by the change but declared nowhere (a part's insides it brings): faded in, never popping in.
  // What comes in or flies has its own way in; what it holds comes with it.
  const coming = [...p.enter.map((m) => after.els[m.after]), ...p.fly.map((f) => after.els[f.after])];
  const before_ = options.from ? null : before.drawn;
  for (const el of before_ ? after.drawn.keys() : []) {
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
    rescroll(c);
    copies.push(c);
    tracks.push({ el: c, frames: m.frames.map(frame), span: m.span });
  }

  // Taken away by the change but declared nowhere (a part's insides it hides), while its part stays: its drawing,
  // from the part's copy made while it was drawn, fades out where it was, never vanishing at once.
  const going = new Set([...p.leave.map((m) => m.before), ...p.fly.map((f) => f.before)]);
  const gone = (j: number | undefined): boolean => j !== undefined && (going.has(j) || gone(before.parts[j].parent));
  for (const [el, place] of before_ ?? []) {
    if (after.drawn.has(el) || place.part === undefined || gone(place.part) || !place.path.length) continue;
    const up = el.parentElement;
    if (up && before_!.has(up) && !after.drawn.has(up) && before_!.get(up)!.part === place.part) continue;
    const copy = before.clones[place.part];
    if (!copy || !full(copy)) continue;
    let node: Element | undefined = copy;
    for (const k of place.path) node = node?.children[k];
    if (!(node instanceof HTMLElement)) continue;
    const c = node.cloneNode(true) as HTMLElement;
    const b = place.box;
    Object.assign(c.style, { position: at ? 'absolute' : 'fixed', left: `${at ? b.left - at.left / z : b.left}px`, top: `${at ? b.top - at.top / z : b.top}px`, width: `${b.width}px`, height: `${b.height}px`, margin: '0', boxSizing: 'border-box' });
    c.inert = true;
    if (under) under.append(c);
    else motionLayer().append(c);
    rescroll(c);
    copies.push(c);
    tracks.push({ el: c, frames: [{ opacity: 1 }, { opacity: 0 }], span: spans?.leave });
  }

  // Rewrapped words: the old drawing (from its part's copy) travels with them and fades out as the new fades in.
  for (const el of rewrapped) {
    const place = before.drawn.get(el)!;
    const copy = place.part === undefined ? null : before.clones[place.part];
    let node: Element | undefined | null = copy && full(copy) ? copy : null;
    for (const k of node ? place.path : []) node = node?.children[k];
    if (!(node instanceof HTMLElement)) continue;
    const c = node.cloneNode(true) as HTMLElement;
    const [b, a] = [place.box, after.drawn.get(el)!.box];
    Object.assign(c.style, { position: at ? 'absolute' : 'fixed', left: `${at ? b.left - at.left / z : b.left}px`, top: `${at ? b.top - at.top / z : b.top}px`, width: `${b.width}px`, height: `${b.height}px`, margin: '0', boxSizing: 'border-box', transformOrigin: '0 0' });
    c.inert = true;
    if (under) under.append(c);
    else motionLayer().append(c);
    rescroll(c);
    copies.push(c);
    tracks.push({ el: c, frames: [{ transform: 'none', opacity: 1 }, { transform: `translate(${a.left - b.left}px, ${a.top - b.top}px)`, opacity: 0 }] });
    tracks.push({ el: el as HTMLElement, frames: [{ opacity: 0 }, { opacity: 1 }] });
  }

  const t = new Transition(still ? tracks.filter((k) => !still.contains(k.el)) : tracks, () => {
    const landed = t.heading === 1;
    for (const f of flights) f.done(landed);
    for (const c of copies) c.remove();
    for (const [el, o] of origins) el.style.transformOrigin = o;
    for (const [el, { overflow, x, y }] of unclipped) {
      el.style.overflow = overflow;
      if (!x && !y) continue;
      for (const c of el.children) (c as HTMLElement).style.translate = '';
      Object.assign(el, { scrollLeft: x, scrollTop: y });
    }
    running.delete(t);
    ended?.(landed);
  }) as Change;
  // On screen, where the finger moves: boxes are in CSS pixels, which the root's zoom scales.
  t.distance = p.distance * z;
  running.set(t, root);
  return t;
}
