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

/** Svelte transition for the dimming behind a sheet or menu. */
export function dim(_node: HTMLElement): TransitionConfig {
  return { duration: still() ? 0 : 200, css: (t) => `opacity: ${t}` };
}
