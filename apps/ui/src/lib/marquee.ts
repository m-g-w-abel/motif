/**
 * One-line text that doesn't fit scrolls once to its end and back, then
 * stops: nothing runs while idle (the WebKitGTK rule against long-running
 * animations). It waits `WAIT` after the text changes; a tap replays it.
 */

import { CURVE, DURATION } from './motion/transition';

/** Pixels a second. */
export const SPEED = 40;
/** The shortest scroll, so a few pixels don't jerk. */
export const MIN_MS = 600;
/** Before scrolling, and at the end before coming back. */
export const WAIT = 2000;
export const PAUSE = 1500;

/** How far and how long to scroll text that overflows its box by `overflow` pixels, or null if it fits. */
export function scrollPlan(overflow: number): { distance: number; ms: number } | null {
  if (overflow <= 1) return null;
  return { distance: overflow, ms: Math.max(MIN_MS, Math.round((overflow / SPEED) * 1000)) };
}

/**
 * Svelte action on a clipping one-line box whose first child holds the text
 * (an inline-block). `key` names what is shown (the track); a new key plays
 * it again after `WAIT`; `false` holds it where it is (the sheet moving: a
 * change starts from the text as drawn), and null stops it and puts it back
 * (the sheet closed, unseen).
 */
export function marquee(node: HTMLElement, key: string | false | null) {
  let timers: ReturnType<typeof setTimeout>[] = [];
  const text = () => node.firstElementChild as HTMLElement | null;

  /** Stops scrolling; `reset` also puts the text back at once (else it stays where it is, as a change measured it). */
  function stop(reset: boolean) {
    timers.forEach(clearTimeout);
    timers = [];
    const el = text();
    if (el && reset) {
      el.style.transition = '';
      el.style.transform = '';
    }
  }

  function play(delay: number) {
    stop(false);
    // Left scrolled (it stopped as the sheet moved): it glides back first, never jumping.
    const el = text();
    if (el?.style.transform) {
      el.style.transition = `transform ${DURATION}ms ${CURVE.css}`;
      el.style.transform = '';
    }
    timers.push(
      setTimeout(() => {
        const el = text();
        const plan = el && scrollPlan(el.offsetWidth - node.clientWidth);
        if (!el || !plan) return;
        el.style.transition = `transform ${plan.ms}ms linear`;
        el.style.transform = `translateX(${-plan.distance}px)`;
        timers.push(setTimeout(() => (el.style.transform = ''), plan.ms + PAUSE));
      }, delay),
    );
  }

  const tap = () => typeof key === 'string' && play(0);
  node.addEventListener('click', tap);
  if (typeof key === 'string') play(WAIT);

  return {
    update(next: string | false | null) {
      if (next === key) return;
      key = next;
      if (typeof key === 'string') play(WAIT);
      else stop(key === null);
    },
    destroy() {
      stop(true);
      node.removeEventListener('click', tap);
    },
  };
}
