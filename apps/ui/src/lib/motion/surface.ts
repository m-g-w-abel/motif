import type { TransitionConfig } from 'svelte/transition';
import { CURVE, DURATION, still } from './transition';

/** Reduced motion: a quicker fade, as `Transition` does. */
const STILL_DURATION = 120;

/**
 * Svelte `in:`/`out:`/`transition:` for something an `{#if}` brings and
 * takes away (docs/ANIMATION.md, Declaring it), on the same clock and
 * curve as every transition:
 * - `below`: a sheet along the bottom of a phone's screen rises from below
 *   and sinks back; wider, it is a popover, and fades;
 * - `grow`: a dialog grows a little into place and fades, and shrinks away;
 * - `fade`: a scrim dims what lies under it.
 * A native `<dialog>` its form has closed already opens again (no longer
 * from the form) while it goes.
 */
export function surface(node: HTMLElement, { enter }: { enter: 'below' | 'grow' | 'fade' }, options?: { direction?: 'in' | 'out' | 'both' }): TransitionConfig {
  if (options?.direction === 'out' && node instanceof HTMLDialogElement && !node.open) node.showModal();
  const easing = CURVE.at;
  if (still()) return { duration: STILL_DURATION, css: (t) => `opacity: ${t}` };
  if (enter === 'below' && matchMedia('(max-width: 700px)').matches) return { duration: DURATION, easing, css: (_t, u) => `transform: translateY(${u * 100}%)` };
  if (enter === 'grow') return { duration: DURATION, easing, css: (t) => `opacity: ${t}; transform: scale(${0.95 + 0.05 * t})` };
  return { duration: DURATION, easing, css: (t) => `opacity: ${t}` };
}
