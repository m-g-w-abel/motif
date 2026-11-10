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
  if (still()) return { duration: 0 };
  left = new Map();
  leftAt = performance.now();
  for (const el of node.querySelectorAll<HTMLElement>('[data-shared]'))
    if (el.offsetWidth) left.set(el.dataset.shared!, { el, box: el.getBoundingClientRect(), font: parseFloat(getComputedStyle(el).fontSize) });
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

/** Svelte `in:` transition for the arriving screen: it fades in, mostly once the leaving one has gone. */
export function arrive(_node: HTMLElement): TransitionConfig {
  if (still()) return { duration: 0 };
  return { duration: DURATION, css: (t) => `opacity: ${t * t}` };
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

function fly(from: Left, el: HTMLElement) {
  const to = el.getBoundingClientRect();
  if (!to.width || !from.el.isConnected) return;
  const text = !el.firstElementChild;
  const of = text ? el : from.el;
  const ghost = of.cloneNode(true) as HTMLElement;
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
    });
  place(1);
  document.body.append(ghost);
  // The window's zoom (Settings) can make CSS pixels differ from measured ones.
  const k = start.width / ghost.getBoundingClientRect().width || 1;
  if (Math.abs(k - 1) > 0.01) place(k);
  const font = parseFloat(getComputedStyle(el).fontSize);
  const move = text ? flip(from.box, to, from.font / font) : flip(to, from.box);
  const transform = `translate(${move.dx * k}px, ${move.dy * k}px) scale(${move.sx}, ${move.sy})`;
  from.el.style.visibility = el.style.visibility = 'hidden';
  // Text starts over the old place and travels home; a copy of the old element travels to the new one.
  ghost.style.transform = text ? transform : '';
  void ghost.offsetWidth;
  ghost.style.transition = `transform ${DURATION}ms ${CURVE}`;
  ghost.style.transform = text ? '' : transform;
  setTimeout(() => {
    el.style.visibility = '';
    ghost.remove();
  }, DURATION);
}
