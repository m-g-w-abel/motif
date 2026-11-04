/**
 * Touch gestures (touch and pen only; the mouse keeps its clicks and drags).
 * Each surface maps its gestures to actions in one table, which the
 * remapping settings (#129) override.
 */

/** Movement before a press counts as a drag, which also picks its axis. */
export const SLOP = 10;
/** The least sideways travel of a swipe. */
export const SWIPE = 50;
/** How long a still press takes to become a long-press. */
export const HOLD = 500;

export type Gesture = 'swipe-left' | 'swipe-right' | 'long-press' | 'drag-vertical';
export type Surface = 'bar' | 'artwork' | 'row';

/** Actions done once, when the gesture completes. */
type Once = 'next' | 'previous' | 'queue';
/** Actions that follow the finger while it drags. */
type Follow = 'volume';
export type Action = Once | Follow;

export const GESTURES: Record<Surface, Partial<Record<Gesture, Action>>> = {
  bar: { 'swipe-left': 'next', 'swipe-right': 'previous' },
  artwork: { 'swipe-left': 'next', 'swipe-right': 'previous', 'drag-vertical': 'volume' },
  row: { 'long-press': 'queue' },
};

/** One press, from where (px) and when (ms) it started. */
export class Press {
  axis: 'x' | 'y' | null = null;

  constructor(
    readonly x: number,
    readonly y: number,
    readonly t: number,
  ) {}

  /** Feeds a move; once it passes SLOP, the axis it mostly ran along is fixed. */
  move(x: number, y: number) {
    const dx = x - this.x;
    const dy = y - this.y;
    if (this.axis === null && Math.hypot(dx, dy) >= SLOP) this.axis = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y';
    return this.axis;
  }

  /** Held still for HOLD by time `t`. */
  held(t: number) {
    return this.axis === null && t - this.t >= HOLD;
  }

  /** How far content follows the finger sideways at `x`: only once the press is a horizontal drag. */
  offset(x: number) {
    return this.axis === 'x' ? x - this.x : 0;
  }

  /** The swipe a release at `x` completes, if any. */
  swipe(x: number): Gesture | null {
    const dx = x - this.x;
    if (this.axis !== 'x' || Math.abs(dx) < SWIPE) return null;
    return dx < 0 ? 'swipe-left' : 'swipe-right';
  }
}

/** A 0..1 value dragged from `start`: up raises it, by the whole range over `span` px. */
export function dragValue(start: number, dy: number, span: number) {
  return Math.min(1, Math.max(0, start - dy / span));
}

/** What a surface can do; a `Follow` action starts a drag and gets its vertical distance (down positive). */
export type Handlers = Partial<Record<Once, () => void> & Record<Follow, () => (dy: number) => void>>;

/** Shows a swipe under way: `move` as the finger goes, then `end` with the swipe it completed (null: none). */
export interface Slide {
  move(dx: number): void;
  end(swipe: Gesture | null): void;
}

/**
 * Moves `el` with a swipe by `transform` alone, in short transitions: it
 * follows the finger, then slides out the swipe's way and back in from the
 * other side (the next track), or springs back.
 */
export function slide(el: HTMLElement): Slide {
  const to = (x: number, ms: number) => {
    el.style.transition = ms ? `transform ${ms}ms ease-out` : 'none';
    el.style.transform = x ? `translateX(${x}px)` : '';
  };
  return {
    move: (dx) => to(dx, 0),
    end(swipe) {
      if (!swipe) return to(0, 200);
      const out = swipe === 'swipe-left' ? -el.offsetWidth : el.offsetWidth;
      to(out, 150);
      setTimeout(() => {
        to(-out, 0);
        void el.offsetWidth; // lays out the jump, so the slide in starts from it
        to(0, 200);
      }, 150);
    },
  };
}

/**
 * Tracks a touch or pen press on `surface` and runs what its gestures map
 * to; `shown` shows its swipes. On surfaces with a long-press, the browser's
 * own long-press (the context menu) is held back; menus stay on right-click.
 */
export function gesture(e: PointerEvent, surface: Surface, on: Handlers, shown?: Slide) {
  if (e.pointerType === 'mouse' || !e.isPrimary) return;
  const map = GESTURES[surface];
  const press = new Press(e.clientX, e.clientY, performance.now());
  const holds = !!map['long-press'];
  let follow: ((dy: number) => void) | undefined;
  let longPressed = false;
  // Its own function per press, so one press's cleanup never removes another's.
  const swallow = (ev: Event) => {
    ev.preventDefault();
    ev.stopPropagation();
  };

  const run = (g: Gesture) => {
    const action = map[g];
    if (action && action !== 'volume') on[action]?.();
  };
  const timer = holds
    ? setTimeout(() => {
        if (!press.held(performance.now())) return;
        longPressed = true;
        run('long-press');
      }, HOLD)
    : undefined;

  const move = (ev: PointerEvent) => {
    if (ev.pointerId !== e.pointerId) return;
    const axis = press.move(ev.clientX, ev.clientY);
    if (axis === 'x') shown?.move(press.offset(ev.clientX));
    if (axis !== 'y') return;
    const action = map['drag-vertical'];
    if (action === 'volume') {
      follow ??= on.volume?.();
      follow?.(ev.clientY - press.y);
    }
  };
  const up = (ev: PointerEvent) => {
    if (ev.pointerId !== e.pointerId) return;
    const swipe = longPressed ? null : press.swipe(ev.clientX);
    if (swipe) run(swipe);
    if (press.axis === 'x') shown?.end(swipe);
    end();
  };
  const end = (ev?: PointerEvent) => {
    if (ev?.type === 'pointercancel' && press.axis === 'x') shown?.end(null);
    clearTimeout(timer);
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', end);
    if (!holds) return;
    // The click (after a long-press) and context menu arrive after the release.
    if (longPressed) window.addEventListener('click', swallow, true);
    setTimeout(() => {
      window.removeEventListener('click', swallow, true);
      window.removeEventListener('contextmenu', swallow, true);
    }, 400);
  };

  if (holds) window.addEventListener('contextmenu', swallow, true);
  window.addEventListener('pointermove', move);
  window.addEventListener('pointerup', up);
  window.addEventListener('pointercancel', end);
}
