import type { TransitionConfig } from 'svelte/transition';
import type { Box } from './fit';
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
 * - `fade`: a scrim dims what lies under it;
 * - with `from` (phones): the element `from` (a tapped button) grows and
 *   moves into this one's box, corner radius and background, and shrinks
 *   back into it as it goes (`morphAt`).
 * A native `<dialog>` its form has closed already opens again (no longer
 * from the form) while it goes.
 */
export function surface(
  node: HTMLElement,
  { enter, from }: { enter: 'below' | 'grow' | 'fade'; from?: HTMLElement },
  options?: { direction?: 'in' | 'out' | 'both' },
): TransitionConfig {
  if (options?.direction === 'out' && node instanceof HTMLDialogElement && !node.open) node.showModal();
  const easing = CURVE.at;
  if (still()) return { duration: STILL_DURATION, css: (t) => `opacity: ${t}` };
  if (from?.isConnected && matchMedia('(max-width: 700px)').matches) {
    const [a, b] = [from.getBoundingClientRect(), node.getBoundingClientRect()];
    // Screen pixels to the node's own (the root's zoom).
    const k = node.offsetWidth / b.width || 1;
    const local = (r: DOMRect): Box => ({ left: r.left * k, top: r.top * k, width: r.width * k, height: r.height * k });
    const radius = (el: Element) => parseFloat(getComputedStyle(el).borderTopLeftRadius) || 0;
    const [ra, rb] = [radius(from), radius(node)];
    return {
      duration: DURATION,
      easing,
      css: (t) => {
        const m = morphAt(local(a), ra, local(b), rb, t);
        // Its background fades in over the button's in the first third.
        return `opacity: ${Math.min(1, t * 3)}; transform: translate(${m.x}px, ${m.y}px); clip-path: inset(${m.inset.map((v) => `${v}px`).join(' ')} round ${m.radius}px)`;
      },
    };
  }
  if (enter === 'below' && matchMedia('(max-width: 700px)').matches) return { duration: DURATION, easing, css: (_t, u) => `transform: translateY(${u * 100}%)` };
  if (enter === 'grow') return { duration: DURATION, easing, css: (t) => `opacity: ${t}; transform: scale(${0.95 + 0.05 * t})` };
  return { duration: DURATION, easing, css: (t) => `opacity: ${t}` };
}

/**
 * A box growing from `from` (radius `ra`) into `to` (radius `rb`) at progress
 * `t`: `to` moved by (x, y) and cut by `inset` (top, right, bottom, left) and
 * `radius`. A translate and a clip, never a scale, so what it holds keeps its
 * size, centred in the box as it grows.
 */
export function morphAt(from: Box, ra: number, to: Box, rb: number, t: number) {
  const mix = (a: number, b: number) => a + (b - a) * t;
  const [w, h] = [mix(from.width, to.width), mix(from.height, to.height)];
  const [x, y] = [mix(from.left, to.left) + w / 2 - (to.left + to.width / 2), mix(from.top, to.top) + h / 2 - (to.top + to.height / 2)];
  const [dx, dy] = [(to.width - w) / 2, (to.height - h) / 2];
  return { x, y, inset: [dy, dx, dy, dx], radius: mix(ra, rb) };
}
