/**
 * One-line text that doesn't fit scrolls once to its end and back, then
 * stops: nothing runs while idle (the WebKitGTK rule against long-running
 * animations). It waits `WAIT` after the text changes; a tap replays it.
 */

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
 * it again after `WAIT`, and null stops it (the sheet is closed).
 */
export function marquee(node: HTMLElement, key: string | null) {
  let timers: ReturnType<typeof setTimeout>[] = [];
  const text = () => node.firstElementChild as HTMLElement | null;

  function stop() {
    timers.forEach(clearTimeout);
    timers = [];
    const el = text();
    if (el) {
      el.style.transition = '';
      el.style.transform = '';
    }
  }

  function play(delay: number) {
    stop();
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

  const tap = () => key !== null && play(0);
  node.addEventListener('click', tap);
  if (key !== null) play(WAIT);

  return {
    update(next: string | null) {
      if (next === key) return;
      key = next;
      if (key === null) stop();
      else play(WAIT);
    },
    destroy() {
      stop();
      node.removeEventListener('click', tap);
    },
  };
}
