import { follow, Transition, type Drive } from './motion/transition';

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
/** `queued` is a row in the queue; `view` is the music area (any view or Settings). */
export type Surface = 'bar' | 'artwork' | 'row' | 'queued' | 'view';

/**
 * Actions done once, when the gesture completes. `close` is the sheet's own
 * drag (down closes now playing, up shows the queue), so the artwork leaves it be.
 */
type Once = 'next' | 'previous' | 'queue' | 'play-next' | 'remove' | 'back' | 'close';
/** Actions that follow the finger while it drags. */
type Follow = 'volume' | 'seek';
export type Action = Once | Follow;
export type GestureMap = Partial<Record<Gesture, Action>>;

export const GESTURES: Record<Surface, GestureMap> = {
  bar: { 'swipe-left': 'next', 'swipe-right': 'previous' },
  artwork: { 'swipe-left': 'next', 'swipe-right': 'previous', 'drag-vertical': 'close' },
  row: { 'long-press': 'queue', 'swipe-left': 'queue' },
  queued: { 'swipe-left': 'remove' },
  view: { 'swipe-right': 'back' },
};

const SKIP: Action[] = ['next', 'previous'];
const QUEUE: Action[] = ['queue', 'play-next'];
/** What each gesture may be remapped to in Settings, besides none. */
export const CHOICES: Record<Surface, Partial<Record<Gesture, Action[]>>> = {
  bar: { 'swipe-left': SKIP, 'swipe-right': SKIP },
  artwork: { 'swipe-left': SKIP, 'swipe-right': SKIP, 'drag-vertical': ['close', 'volume', 'seek'] },
  row: { 'long-press': QUEUE, 'swipe-left': QUEUE },
  queued: { 'swipe-left': ['remove'] },
  view: { 'swipe-right': ['back'] },
};

/** The user's remappings (Settings), per surface: an action, or none to turn the gesture off. */
export type Overrides = Partial<Record<Surface, Partial<Record<Gesture, Action | 'none'>>>>;

/** A surface's gestures with `overrides` applied; anything the gesture doesn't offer (from storage) is ignored. */
export function mapped(overrides: unknown, surface: Surface): GestureMap {
  const map = { ...GESTURES[surface] };
  const own: unknown = (overrides as Record<string, unknown> | null)?.[surface];
  if (typeof own !== 'object' || own === null) return map;
  for (const [g, choices] of Object.entries(CHOICES[surface]) as [Gesture, Action[]][]) {
    const action = (own as Record<string, unknown>)[g];
    if (action === 'none') delete map[g];
    else if (choices.includes(action as Action)) map[g] = action as Action;
  }
  return map;
}

/** One press, from where (px) and when (ms) it started. */
export class Press {
  axis: 'x' | 'y' | null = null;
  /** Where the finger was when the press became a drag. */
  #from = 0;

  constructor(
    readonly x: number,
    readonly y: number,
    readonly t: number,
  ) {}

  /** Feeds a move; once it passes SLOP, the axis it mostly ran along is fixed. */
  move(x: number, y: number) {
    const dx = x - this.x;
    const dy = y - this.y;
    if (this.axis === null && Math.hypot(dx, dy) >= SLOP) {
      this.axis = Math.abs(dx) > Math.abs(dy) ? 'x' : 'y';
      this.#from = x;
    }
    return this.axis;
  }

  /** Held still for HOLD by time `t`. */
  held(t: number) {
    return this.axis === null && t - this.t >= HOLD;
  }

  /** How far content follows the finger sideways at `x`: only once the press is a horizontal drag, from where it became one (it starts where it is, never jumping by the slop). */
  offset(x: number) {
    return this.axis === 'x' ? x - this.#from : 0;
  }

  /** The swipe a release at `x` completes, if any. */
  swipe(x: number): Gesture | null {
    const dx = x - this.x;
    if (this.axis !== 'x' || Math.abs(dx) < SWIPE) return null;
    return dx < 0 ? 'swipe-left' : 'swipe-right';
  }
}

const isFollow = (a: Action): a is Follow => a === 'volume' || a === 'seek';

/** A 0..1 value dragged from `start`: up raises it, by the whole range over `span` px. */
export function dragValue(start: number, dy: number, span: number) {
  return Math.min(1, Math.max(0, start - dy / span));
}

/** What a surface can do; a `Follow` action starts a drag (undefined: not now) and gets its vertical distance (down positive). */
export type Handlers = Partial<Record<Once, () => void> & Record<Follow, () => ((dy: number) => void) | undefined>>;

/**
 * Draws a sideways swipe as it goes: for its way (-1 left, 1 right), a
 * transition the finger drives across `distance` px on screen (where the finger
 * moves, the root's zoom included), then releases (null:
 * nothing moves that way).
 */
export type Swipe = (way: -1 | 1) => { drive: Drive; distance: number } | null;

const translate = (x: number) => ({ transform: x ? `translateX(${x}px)` : 'none' });

/**
 * Moves `el` with a swipe (the bar's track, the sheet's artwork): it
 * follows the finger out, then on letting go either goes on out and the
 * next track comes in from the other side, or springs back.
 */
export function slide(el: HTMLElement): Swipe {
  return (way) => {
    const w = el.offsetWidth;
    const t = new Transition([{ el, frames: [translate(0), translate(way * w)] }]);
    const drive = follow(t);
    return {
      distance: el.getBoundingClientRect().width,
      drive: {
        seek: (p) => drive.seek(p),
        release(commit) {
          drive.release(commit);
          void t.done.then(() => {
            // In the same task as the first one's clean-up, so it never shows where it started.
            if (commit) void new Transition([{ el, frames: [translate(-way * w), translate(0)] }]).play(1);
          });
        },
      },
    };
  };
}

/**
 * Moves a list row left with a swipe over a strip that shows what the swipe
 * does: a green + (`queue`) or a red × (`remove`), uncovered as the row
 * goes. A queued row slides out to the left and comes back the same way; a
 * removed one stays out, coming back only if still shown a while later (the
 * engine refused). The row's parent clips both while it moves; a table row
 * has none to spare and slides without the strip.
 */
export function swipeRow(el: HTMLElement, does: 'queue' | 'remove'): Swipe {
  return (way) => {
    if (way > 0) return null;
    const box = el instanceof HTMLTableRowElement ? null : el.parentElement;
    const w = el.offsetWidth;
    const kept = box?.style.cssText ?? '';
    let strip: HTMLElement | undefined;
    if (box) {
      box.style.overflow = 'hidden';
      if (getComputedStyle(box).position === 'static') box.style.position = 'relative';
      strip = document.createElement('span');
      strip.className = `swipe-strip ${does}`;
      strip.textContent = does === 'queue' ? '+' : '×';
      strip.ariaHidden = 'true';
      // Level with the row, which needn't fill its parent (a queue's row among the others).
      Object.assign(strip.style, { top: `${el.offsetTop}px`, bottom: 'auto', height: `${el.offsetHeight}px` });
      box.append(strip);
    }
    /** The row (and strip) from `a` px to `b` px. */
    const move = (a: number, b: number) =>
      new Transition([
        { el, frames: [translate(a), translate(b)] },
        ...(strip ? [{ el: strip, frames: [{ transform: `translateX(calc(100% + ${a}px))` }, { transform: `translateX(calc(100% + ${b}px))` }] }] : []),
      ]);
    const done = () => {
      strip?.remove();
      if (box) box.style.cssText = kept;
    };
    const back = () => void move(-w, 0).play(1).then(done);
    const t = move(0, -w);
    const drive = follow(t);
    return {
      distance: el.getBoundingClientRect().width,
      drive: {
        seek: (p) => drive.seek(p),
        release(commit) {
          drive.release(commit);
          if (!commit) return void t.done.then(done);
          /** Out of sight until it comes back (or goes, removed). */
          const out = () => {
            el.style.transform = translate(-w).transform;
            if (strip) strip.style.transform = `translateX(calc(100% - ${w}px))`;
          };
          const restore = () => {
            el.style.transform = '';
            back();
          };
          if (does === 'queue') return void t.done.then(() => (out(), setTimeout(restore, 100)));
          // Removed: the strip closes up as the rows below move up into the row's place (the list's own change, same
          // clock), from where it is drawn: the row can go while it is still sliding out (a short swipe).
          let refused: ReturnType<typeof setTimeout> | undefined;
          const closes = new MutationObserver(() => {
            if (el.isConnected) return;
            closes.disconnect();
            clearTimeout(refused);
            if (!strip?.isConnected) return done();
            const at = getComputedStyle(strip).transform;
            strip.style.transformOrigin = 'top';
            void new Transition([{ el: strip, frames: [{ transform: `${at} scaleY(1)` }, { transform: `${at} scaleY(0)` }] }]).play(1).then(done);
          });
          if (box) closes.observe(box, { childList: true });
          void t.done.then(() => {
            if (!el.isConnected) return;
            out();
            refused = setTimeout(() => {
              closes.disconnect();
              if (el.isConnected) restore();
              else done();
            }, 1000);
          });
        },
      },
    };
  };
}

/**
 * Makes the touch under way ours alone until the returned function is
 * called: the browser's own handling of a drag ends in a fling, and
 * Chromium drops the next tap while one runs (nothing could be tapped for
 * half a second after a drag), even where `touch-action` rules out panning.
 * With `when`, only the moves it picks are ours (the rest may pan).
 */
export function ownTouch(when: (ev: TouchEvent) => boolean = () => true): () => void {
  const own = (ev: TouchEvent) => ev.cancelable && when(ev) && ev.preventDefault();
  window.addEventListener('touchmove', own, { passive: false });
  return () => window.removeEventListener('touchmove', own);
}

/**
 * Tracks a touch or pen press on a surface and runs what its gestures map
 * to (`mapped`); `shown` shows its swipes that do something. On surfaces with a long-press, the browser's
 * own long-press (the context menu) is held back; menus stay on right-click.
 */
export function gesture(e: PointerEvent, map: GestureMap, on: Handlers, shown?: Swipe) {
  if (e.pointerType === 'mouse' || !e.isPrimary) return;
  const press = new Press(e.clientX, e.clientY, performance.now());
  const holds = !!map['long-press'];
  const swipes = !!(map['swipe-left'] || map['swipe-right']);
  let follows: ((dy: number) => void) | undefined;
  let longPressed = false;
  /** The swipe drawn, and the way it went first. */
  let showing: ReturnType<Swipe> = null;
  let swiping = 0;
  // Its own function per press, so one press's cleanup never removes another's.
  const swallow = (ev: Event) => {
    ev.preventDefault();
    ev.stopPropagation();
  };

  const run = (g: Gesture) => {
    const action = map[g];
    if (action && !isFollow(action)) on[action]?.();
  };
  const timer = holds
    ? setTimeout(() => {
        if (!press.held(performance.now())) return;
        longPressed = true;
        run('long-press');
      }, HOLD)
    : undefined;

  // A sideways drag that is a swipe is ours alone: the browser's own handling of the touch would end in a
  // fling, and Chromium drops the next tap while one runs (nothing could be tapped for half a second).
  const own = (ev: TouchEvent) => press.axis === 'x' && swipes && ev.cancelable && ev.preventDefault();
  const move = (ev: PointerEvent) => {
    if (ev.pointerId !== e.pointerId) return;
    const axis = press.move(ev.clientX, ev.clientY);
    // Only the ways that do something follow the finger (a row moves left to queue, not right); the first way it goes, it keeps.
    const dx = press.offset(ev.clientX);
    if (axis === 'x' && swipes && dx) {
      const way = dx < 0 ? -1 : 1;
      if (!swiping && map[way < 0 ? 'swipe-left' : 'swipe-right']) {
        swiping = way;
        showing = shown?.(way) ?? null;
      }
      if (showing) showing.drive.seek(Math.max(0, dx * swiping) / showing.distance);
    }
    if (axis !== 'y') return;
    const action = map['drag-vertical'];
    if (action && isFollow(action)) {
      follows ??= on[action]?.();
      follows?.(ev.clientY - press.y);
    }
  };
  const up = (ev: PointerEvent) => {
    if (ev.pointerId !== e.pointerId) return;
    const swipe = longPressed ? null : press.swipe(ev.clientX);
    if (swipe) run(swipe);
    showing?.drive.release(!!swipe && !!map[swipe] && (swipe === 'swipe-left') === (swiping < 0));
    end();
  };
  const end = (ev?: PointerEvent) => {
    if (ev?.type === 'pointercancel') showing?.drive.release(false);
    clearTimeout(timer);
    window.removeEventListener('pointermove', move);
    window.removeEventListener('pointerup', up);
    window.removeEventListener('pointercancel', end);
    window.removeEventListener('touchmove', own);
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
  window.addEventListener('touchmove', own, { passive: false });
  window.addEventListener('pointerup', up);
  window.addEventListener('pointercancel', end);
}
