import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';

/**
 * Transitions not yet moved onto `lib/motion` (docs/ANIMATION.md, Migration):
 * sheets and menus, dialogs, content swaps, the queue's rows, and the
 * now-playing sheet's scrubbed step changes.
 */

const CURVE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';
const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;

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
