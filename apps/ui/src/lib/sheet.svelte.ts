import { SLOP } from './gestures';
import type { Transition } from './motion/transition';

/**
 * What the open sheet shows: now playing, the queue under the controls
 * (half) or filling the screen (full), or the lyrics. Desktop uses only now
 * playing and lyrics.
 */
export type SheetStep = 'now' | 'half' | 'full' | 'lyrics';

/** Pixels a drag must go to step the sheet (phones). */
export const STEP = 48;

/** What letting go of a handle dragged `dy` pixels does: a tap, a step, or nothing. */
export function released(dy: number): 'tap' | 'up' | 'back' | null {
  if (Math.abs(dy) < SLOP) return 'tap';
  if (dy <= -STEP) return 'up';
  return dy >= STEP ? 'back' : null;
}

/** The step a drag or a press goes to: up towards the full queue, or back towards now playing and then closed. */
export function stepTo(step: SheetStep, way: 'up' | 'back'): SheetStep | 'closed' | null {
  if (way === 'up') return step === 'now' ? 'half' : step === 'half' ? 'full' : null;
  return step === 'now' ? 'closed' : step === 'full' ? 'half' : 'now';
}

/** What the sheet draws its changes with (`ExpandedPlayer`). */
export interface SheetMotion {
  /** The open/close transition, from the bar (0) to the sheet (1), with the sheet drawn; null: none to draw (no bar, reduced motion). */
  toggle(): Transition | null;
  /** Changes the step by `apply`, drawn as a transition. */
  step(apply: () => void): void;
}

/**
 * The expanded now-playing sheet, which grows out of the player bar. Opening
 * and closing is one transition (docs/ANIMATION.md, The now-playing sheet):
 * a tap plays it, a drag seeks it and letting go plays it on or back, and a
 * drag that catches it moving carries on from where it is drawn.
 */
export class PlayerSheet {
  /** The state: open over everything, or not. */
  open = $state(false);
  /** Drawn on its way open or closed (a transition, or a finger moving it). */
  moving = $state(false);
  step = $state<SheetStep>('now');
  /** The bar or floating player the sheet grows out of, when one is shown. */
  origin: HTMLElement | null = null;
  /** Set by the sheet. */
  motion: SheetMotion | null = null;
  #t: Transition | null = null;

  /** Open and still: what moves only at rest (marquees, the carousel's side cards) may. */
  get rest() {
    return this.open && !this.moving;
  }

  /** The lyrics view instead of the artwork. */
  get lyrics() {
    return this.step === 'lyrics';
  }

  toggleLyrics() {
    this.#go(this.lyrics ? 'now' : 'lyrics');
  }

  /** Towards the full queue: drag up, or the handle. */
  up() {
    const to = this.open && stepTo(this.step, 'up');
    if (to && to !== 'closed') this.#go(to);
  }

  /** One step back (swipe down, Android back): full → half → now playing → closed. */
  back() {
    if (!this.open) return;
    const to = stepTo(this.step, 'back');
    if (to === 'closed') this.collapse();
    else if (to) this.#go(to);
  }

  #go(step: SheetStep) {
    if (this.motion && this.rest) this.motion.step(() => (this.step = step));
    else this.step = step;
  }

  expand() {
    if (this.rest) return;
    this.#take(0);
    this.#settle(true);
  }

  collapse() {
    if (!this.open && !this.moving) return;
    this.#take(1);
    this.#settle(false);
  }

  /** The open/close transition: the one moving, caught where it is drawn, or a new one at `from`. */
  #take(from: 0 | 1) {
    if (this.#t?.grab() != null) return this.#t;
    this.moving = true;
    const t = this.motion?.toggle() ?? null;
    this.#t = t;
    if (!t) return null;
    t.seek(from);
    void t.done.then(() => {
      if (this.#t !== t) return;
      this.#t = null;
      this.moving = false;
      if (t.heading === 0) this.#closed();
    });
    return t;
  }

  /** Plays the sheet open or closed from where it is drawn. */
  #settle(open: boolean) {
    this.open = open;
    if (this.#t) void this.#t.play(open ? 1 : 0);
    else {
      this.moving = false;
      if (!open) this.#closed();
    }
  }

  /** The queue's steps don't outlast closing; lyrics do. */
  #closed() {
    if (this.step === 'half' || this.step === 'full') this.step = 'now';
  }

  /**
   * Tracks a vertical drag: the sheet follows it, then opens or closes on
   * letting go (a flick by its direction, else by how far it got). `from` is
   * where it started: the bar (dragging up opens) or the sheet (down
   * closes). A tap without movement calls `onTap`. A touch that moves
   * sideways first is a swipe (see `gestures.ts`), not a drag. With `steps`
   * (phones), dragging the open sheet up steps it towards the queue.
   */
  drag(e: PointerEvent, from: 'bar' | 'sheet', onTap?: () => void, steps = false) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    const touch = e.pointerType !== 'mouse';
    const height = window.innerHeight;
    let lastY = startY;
    let lastT = performance.now();
    let velocity = 0;
    let moved = false;
    let swiped = false;
    let t: Transition | null = null;
    let start = 0;
    let p = 0;

    const move = (ev: PointerEvent) => {
      const dy = ev.clientY - startY;
      const dx = ev.clientX - startX;
      if (!moved && touch && Math.abs(dx) >= 8 && Math.abs(dx) > Math.abs(dy)) {
        swiped = true;
        return end();
      }
      if (!moved && Math.abs(dy) < 8) return;
      if (!moved) {
        t = this.#take(this.open ? 1 : 0);
        start = t?.progress ?? (this.open ? 1 : 0);
      }
      moved = true;
      p = Math.min(1, Math.max(0, start - dy / height));
      t?.seek(p);
      const now = performance.now();
      velocity = (ev.clientY - lastY) / Math.max(1, now - lastT);
      lastY = ev.clientY;
      lastT = now;
    };

    const end = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      if (!moved) {
        if (!swiped) onTap?.();
        return;
      }
      // A flick decides by direction; a slow drag by how far it got.
      const flick = Math.abs(velocity) > 0.5;
      const open = flick ? velocity < 0 : p > (from === 'bar' ? 0.25 : 0.75);
      this.#settle(open);
      if (steps && from === 'sheet' && released(lastY - startY) === 'up') this.up();
    };

    // On the window, so a fast flick that leaves the bar still counts.
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }

  /**
   * A drag on one of the phone sheet's handles: up steps towards the full
   * queue, down steps back, a tap calls `onTap`. Keyboard presses reach the
   * handle's click instead.
   */
  handle(e: PointerEvent, onTap: () => void) {
    if (e.button !== 0) return;
    const startY = e.clientY;
    const end = (ev: PointerEvent) => {
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
      if (ev.type === 'pointercancel') return;
      const what = released(ev.clientY - startY);
      if (what === 'tap') onTap();
      else if (what === 'up') this.up();
      else if (what === 'back') this.back();
    };
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }
}
