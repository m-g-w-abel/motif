import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';
import type { Slide } from './gestures';

/**
 * Screen changes in the music area. The leaving screen slides and fades
 * away (`leave`) as the arriving one fades in where it stays (`arrive`: no
 * transform, so its elements can be measured where they will be). Elements on both screens (`shared`: an album's cover and title
 * on its tile and its page) fly from the old place to the new as a copy
 * above both, so neither screen's scrolling clips them and no two drawings
 * of the same text crossfade (#109). Short transitions of transform and
 * opacity only; reduced motion changes screens at once.
 */

/** Into a deeper screen, back out of one, or across to another at the same depth. */
export type Motion = 'forward' | 'back' | 'swap';

export function motion(from: number, to: number): Motion {
  return to > from ? 'forward' : to < from ? 'back' : 'swap';
}

type Box = Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>;

/** FLIP: the transform (origin top left) that puts box `to` over box `from`; text scales evenly by `scale`. */
export function flip(from: Box, to: Box, scale?: number) {
  return { dx: from.left - to.left, dy: from.top - to.top, sx: scale ?? from.width / to.width, sy: scale ?? from.height / to.height };
}

const DURATION = 260;
const CURVE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';
/** How long after a screen leaves its shared elements wait for their counterparts (a grid's cards load). */
const WAIT = 500;
const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

type Left = { el: HTMLElement; box: DOMRect; font: number };
/** The shared elements of the screen that last left, by name. */
let left = new Map<string, Left>();
let leftAt = 0;
/** Shared elements that arrived this frame, in case their screen came before the old one left. */
let arrivals: [string, HTMLElement][] = [];

/**
 * Svelte `out:` transition for a screen giving way: forward it fades to the
 * left, back to the right, across in place. A screen a swipe back has been
 * dragging (`follow`) carries on from where the finger left it, off the side.
 */
export function leave(node: HTMLElement, how: Motion): TransitionConfig {
  node.dataset.leaving = '';
  if (still() || quiet) return { duration: 0 };
  left = new Map();
  leftAt = performance.now();
  for (const el of node.querySelectorAll<HTMLElement>('[data-shared]'))
    if (el.offsetWidth) left.set(el.dataset.shared!, leftOf(el));
  for (const [name, el] of arrivals) match(name, el);
  node.style.pointerEvents = 'none';
  const dragged = Number(/translateX\((-?[\d.]+)px\)/.exec(node.style.transform)?.[1] ?? 0);
  const width = node.offsetWidth;
  const end = dragged ? width : how === 'forward' ? -0.08 * width : how === 'back' ? 0.24 * width : 0;
  return {
    duration: how === 'swap' ? 160 : DURATION,
    easing: cubicOut,
    css: (t, u) => `transform: translateX(${dragged + (end - dragged) * u}px); opacity: ${t}`,
  };
}

/** Moves a screen with a swipe back as the finger goes (rightwards only), springing back if the swipe falls short; `leave` finishes one that went back. */
export function follow(el: HTMLElement): Slide {
  const to = (x: number, ms: number) => {
    el.style.transition = ms ? `transform ${ms}ms ease-out` : 'none';
    el.style.transform = x > 0 ? `translateX(${x}px)` : '';
  };
  return {
    move: (dx) => to(dx, 0),
    end: (swipe) => {
      if (!swipe) to(0, 200);
    },
  };
}

const phone = () => matchMedia('(max-width: 700px)').matches;

/** Svelte transition for a sheet along the bottom of a phone's screen: it rises from below and sinks back. Elsewhere (a popover) it fades. */
export function rise(_node: HTMLElement): TransitionConfig {
  if (still()) return { duration: 0 };
  if (!phone()) return { duration: 120, css: (t) => `opacity: ${t}` };
  return { duration: 240, easing: cubicOut, css: (_t, u) => `transform: translateY(${u * 100}%)` };
}

/**
 * Svelte `in:`/`out:` transition for a dialog: it grows a little into place
 * and fades, and shrinks away. A native `<dialog>` its form has closed
 * already opens again (no longer from the form) while it goes.
 */
export function pop(node: HTMLElement, _params?: unknown, options?: { direction?: 'in' | 'out' | 'both' }): TransitionConfig {
  if (options?.direction === 'out' && node instanceof HTMLDialogElement && !node.open) node.showModal();
  if (still()) return { duration: 0 };
  return { duration: 180, easing: cubicOut, css: (t) => `opacity: ${t}; transform: scale(${0.95 + 0.05 * t})` };
}

/** Shows that what `el` holds has just changed (results for a new search, another list): it rises a little into place. */
export function refresh(el: HTMLElement) {
  if (still()) return;
  el.animate([{ opacity: 0.4, transform: 'translateY(12px)' }, { opacity: 1, transform: 'none' }], { duration: 180, easing: CURVE });
}

/** Svelte transition for the dimming behind a sheet or menu. */
export function dim(_node: HTMLElement): TransitionConfig {
  return { duration: still() ? 0 : 200, css: (t) => `opacity: ${t}` };
}

/**
 * FLIP for rows that move when others come or go (a queue row removed):
 * call it before the change with the rows (each with `data-key`), and the
 * function it returns after, which slides each kept row from its old place.
 */
export function slideRows(rows: () => Iterable<HTMLElement>) {
  if (still()) return () => {};
  const tops = new Map<string, number>();
  for (const el of rows()) tops.set(el.dataset.key!, el.getBoundingClientRect().top);
  return () => {
    const moved: HTMLElement[] = [];
    for (const el of rows()) {
      const top = tops.get(el.dataset.key!);
      const dy = top === undefined ? 0 : top - el.getBoundingClientRect().top;
      if (Math.abs(dy) < 1) continue;
      el.style.transition = 'none';
      el.style.transform = `translateY(${dy}px)`;
      moved.push(el);
    }
    if (!moved.length) return;
    void moved[0].offsetWidth;
    for (const el of moved) {
      el.style.transition = `transform 200ms ${CURVE}`;
      el.style.transform = '';
    }
  };
}

/**
 * Svelte `in:` transition for the arriving screen: it fades in, mostly once
 * the leaving one has gone; going back, it comes in from the left too, as
 * the forward move took it away.
 */
export function arrive(node: HTMLElement, how: () => Motion): TransitionConfig {
  if (still() || quiet) return { duration: 0 };
  const x = how() === 'back' ? -0.08 * node.offsetWidth : 0;
  return { duration: DURATION, easing: cubicOut, css: (t, u) => `transform: translateX(${x * u}px); opacity: ${t * t}` };
}

/** What a copy keeps of its original's look, which came partly from rules on its ancestors. */
const LOOK = ['font-family', 'font-size', 'font-weight', 'font-style', 'line-height', 'letter-spacing', 'color', 'white-space', 'overflow', 'text-overflow', 'border-radius', 'background-color'];

/**
 * Svelte action naming an element that appears on two screens. Arriving on
 * a screen just after one with the same name left, it flies there from the
 * old one's place: text as a copy of itself (the same words, scaled evenly),
 * anything else as a copy of the old one (already drawn; the new artwork may
 * still be loading).
 */
export function shared(el: HTMLElement, name: string) {
  el.dataset.shared = name;
  if (!match(name, el)) {
    if (!arrivals.length) requestAnimationFrame(() => (arrivals = []));
    arrivals.push([name, el]);
  }
  return { update: (next: string) => (el.dataset.shared = next) };
}

function match(name: string, el: HTMLElement) {
  const from = performance.now() - leftAt < WAIT ? left.get(name) : undefined;
  if (!from || from.el === el) return false;
  left.delete(name);
  // A frame on, once the new screen has laid out and scrolled to where it was.
  requestAnimationFrame(() => fly(from, el));
  return true;
}

/** Whether `el` is drawn as it will stay: its artwork (if any) loaded, or settled on its placeholder. */
function drawn(el: HTMLElement) {
  const art = el.matches('.art') ? el : el.querySelector<HTMLElement>('.art');
  if (!art) return true;
  const img = art.querySelector('img');
  return img ? img.complete && img.naturalWidth > 0 : !!art.style.background;
}

/** Shows `el` in place of `ghost` once it is drawn (a second at most), so the copy never gives way to an empty cover. */
function handOver(el: HTMLElement, ghost: HTMLElement, since = performance.now()) {
  if (!drawn(el) && performance.now() - since < 1000) return void requestAnimationFrame(() => handOver(el, ghost, since));
  el.style.visibility = '';
  ghost.remove();
}

/** What `el` would be as a shared element leaving now. */
const leftOf = (el: HTMLElement): Left => ({ el, box: el.getBoundingClientRect(), font: parseFloat(getComputedStyle(el).fontSize) });

/**
 * A copy above everything that carries shared element `from` to `el` (at
 * box `to`): text as a copy of the new one, scaled evenly, anything else as
 * a copy of the old. Returns it with its start and end transforms, both
 * originals hidden; the caller animates it and hands over.
 */
function flight(from: Left, el: HTMLElement, to: DOMRect) {
  const text = !el.firstElementChild;
  const of = text ? el : from.el;
  const ghost = of.cloneNode(true) as HTMLElement;
  // The original's lazy, async image would decode afresh and draw nothing for a frame.
  for (const img of ghost.querySelectorAll('img')) Object.assign(img, { loading: 'eager', decoding: 'sync' });
  const look = getComputedStyle(of);
  for (const p of LOOK) ghost.style.setProperty(p, look.getPropertyValue(p));
  const start = text ? to : from.box;
  const place = (k: number) =>
    Object.assign(ghost.style, {
      position: 'fixed',
      left: `${start.left * k}px`,
      top: `${start.top * k}px`,
      width: `${start.width * k}px`,
      height: `${start.height * k}px`,
      margin: '0',
      zIndex: '30',
      pointerEvents: 'none',
      transformOrigin: '0 0',
      visibility: 'visible',
    });
  place(1);
  document.body.append(ghost);
  // The window's zoom (Settings) can make CSS pixels differ from measured ones.
  const k = start.width / ghost.getBoundingClientRect().width || 1;
  if (Math.abs(k - 1) > 0.01) place(k);
  const font = parseFloat(getComputedStyle(el).fontSize);
  const move = text ? flip(from.box, to, from.font / font) : flip(to, from.box);
  const away = `translate(${move.dx * k}px, ${move.dy * k}px) scale(${move.sx}, ${move.sy})`;
  from.el.style.visibility = el.style.visibility = 'hidden';
  // Text starts over the old place and travels home; a copy of the old element travels to the new one.
  return { ghost, frames: text ? [{ transform: away }, { transform: 'none' }] : [{ transform: 'none' }, { transform: away }] };
}

function fly(from: Left, el: HTMLElement) {
  const box = el.getBoundingClientRect();
  // Where it will be once its screen has slid into place.
  const screen = el.closest<HTMLElement>('.screen');
  const shift = screen ? new DOMMatrix(getComputedStyle(screen).transform).m41 : 0;
  const to = new DOMRect(box.x - shift, box.y, box.width, box.height);
  if (!to.width || !from.el.isConnected) return;
  const { ghost, frames } = flight(from, el, to);
  ghost.animate(frames, { duration: DURATION, easing: CURVE, fill: 'forwards' });
  setTimeout(() => handOver(el, ghost), DURATION);
}

/** Animations a drag scrubs (Web Animations): paused, moved by `seek`, then played on to the end or back to the start. */
export class Scrub {
  constructor(readonly animations: Animation[]) {
    for (const a of animations) a.pause();
  }

  #at = 0;

  /** Puts each animation at `p` (0 to 1) of its length. */
  seek(p: number) {
    this.#at = Math.min(1, Math.max(0, p));
    for (const a of this.animations) this.#put(a);
  }

  /** Joins `a` to the others, where they are now. */
  add(a: Animation) {
    a.pause();
    this.animations.push(a);
    this.#put(a);
  }

  #put(a: Animation) {
    a.currentTime = this.#at * Number(a.effect?.getComputedTiming().duration ?? 0);
  }

  /** Plays on to the end (`forward`) or back to the start, then calls `done` once, all finished. */
  release(forward: boolean, done: () => void) {
    for (const a of this.animations) {
      if (forward) a.play();
      else a.reverse();
    }
    // Cancelled ones (their element gone) count as finished.
    void Promise.allSettled(this.animations.map((a) => a.finished)).then(done);
  }
}

/** Whether the next screen change happens at once: a drag has already shown it (`backScrub`). */
let quiet = false;
/** Changes screens with `change` at once, as a scrubbed drag has already drawn the change. */
export function quietly(change: () => void) {
  quiet = true;
  change();
  requestAnimationFrame(() => requestAnimationFrame(() => (quiet = false)));
}

/**
 * A swipe back's change from `screen` to `under` (the screen it goes back
 * to, drawn beneath, still): the one on top slides off to the right, the one
 * beneath comes in from the left, and their shared elements (an album's
 * cover and title, tile ↔ page) travel between them, all as far as the
 * finger has gone. `drop()` removes the copies once the change is over.
 */
export function backScrub(screen: HTMLElement, under: HTMLElement) {
  const width = screen.offsetWidth;
  const timing: KeyframeAnimationOptions = { duration: DURATION, fill: 'both', easing: 'linear' };
  const ghosts: HTMLElement[] = [];
  const hidden: HTMLElement[] = [];
  const animations: Animation[] = [];
  /** Shared elements still waiting for their counterpart (a card that comes once the grid has scrolled back to it). */
  let waiting = still() ? [] : [...screen.querySelectorAll<HTMLElement>('[data-shared]')].filter((el) => el.offsetWidth).map(leftOf);
  const slide = under.animate([{ transform: `translateX(${-0.08 * width}px)`, opacity: 0.5 }, { transform: 'none', opacity: 1 }], timing);
  const scrub = new Scrub([screen.animate([{ transform: 'none' }, { transform: `translateX(${width}px)` }], timing), slide]);
  animations.push(...scrub.animations);
  /** Pairs what has come: measured where it will be, without the slide it is in. */
  const pair = () => {
    waiting = waiting.filter((from) => {
      const to = under.querySelector<HTMLElement>(`[data-shared="${CSS.escape(from.el.dataset.shared!)}"]`);
      if (!to?.offsetWidth) return true;
      const box = to.getBoundingClientRect();
      const shift = new DOMMatrix(getComputedStyle(under).transform).m41;
      const { ghost, frames } = flight(from, to, new DOMRect(box.x - shift, box.y, box.width, box.height));
      ghosts.push(ghost);
      hidden.push(from.el, to);
      const a = ghost.animate(frames, timing);
      animations.push(a);
      scrub.add(a);
      return false;
    });
  };
  pair();
  return {
    scrub: {
      seek(p: number) {
        if (waiting.length) pair();
        scrub.seek(p);
      },
      release: (forward: boolean, done: () => void) => scrub.release(forward, done),
    },
    /** Back to how things were (the swipe was cancelled), or done with the copies (it went back). */
    drop(restore: boolean) {
      for (const g of ghosts) g.remove();
      if (restore) for (const el of hidden) el.style.visibility = '';
      for (const a of animations) a.cancel();
    },
  };
}
