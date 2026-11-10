import { flushSync } from 'svelte';
import { SLOP } from './gestures';

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

/**
 * The expanded now-playing sheet, which grows out of the player bar.
 * `progress` runs from 0 (collapsed) to 1 (covering the screen); dragging
 * sets it directly, and releasing snaps it to either end.
 */
export class PlayerSheet {
  progress = $state(0);
  /** True while a finger or pointer is moving the sheet, to disable easing. */
  dragging = $state(false);
  step = $state<SheetStep>('now');
  /** The bar or floating player the sheet grows out of, when one is shown. */
  origin: HTMLElement | null = null;
  /** The morph follows a drag down from the sheet closely, not in its stages (`ExpandedPlayer`). */
  scrub = false;
  /** Set by the sheet: measures the morph's start and end boxes. */
  measure?: () => void;

  get open() {
    return this.progress > 0;
  }

  /** The lyrics view instead of the artwork. */
  get lyrics() {
    return this.step === 'lyrics';
  }

  toggleLyrics() {
    this.step = this.lyrics ? 'now' : 'lyrics';
  }

  /** Towards the full queue: drag up, or the handle. */
  up() {
    if (!this.open) return;
    if (this.step === 'now') this.step = 'half';
    else if (this.step === 'half') this.step = 'full';
  }

  /** One step back (swipe down, Android back): full → half → now playing → closed. */
  back() {
    if (!this.open) return;
    if (this.step === 'now') this.collapse();
    else this.step = this.step === 'full' ? 'half' : 'now';
  }

  expand() {
    this.scrub = false;
    this.measure?.();
    this.settle(1);
  }

  collapse() {
    // The morph is laid out for now playing; the queue states don't outlast closing.
    if (this.step === 'half' || this.step === 'full') {
      this.step = 'now';
      flushSync();
    }
    this.scrub = false;
    this.measure?.();
    this.settle(0);
  }

  private settle(progress: number) {
    this.dragging = false;
    this.progress = progress;
  }

  /**
   * Tracks a vertical drag. `direction` is the way that opens the sheet:
   * -1 when dragging up opens it (from the bar), 1 when dragging down closes
   * it (from the sheet). A tap without movement calls `onTap`. A touch that
   * moves sideways first is a swipe (see `gestures.ts`), not a drag. With
   * `steps` (phones), dragging the open sheet up steps it towards the queue.
   */
  drag(e: PointerEvent, from: 'bar' | 'sheet', onTap?: () => void, steps = false) {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    const touch = e.pointerType !== 'mouse';
    const startProgress = this.progress;
    const height = window.innerHeight;
    let lastY = startY;
    let lastT = performance.now();
    let velocity = 0;
    let moved = false;
    let swiped = false;

    const move = (ev: PointerEvent) => {
      const dy = ev.clientY - startY;
      const dx = ev.clientX - startX;
      if (!moved && touch && Math.abs(dx) >= 8 && Math.abs(dx) > Math.abs(dy)) {
        swiped = true;
        return end();
      }
      if (!moved && Math.abs(dy) < 8) return;
      if (!moved) {
        this.scrub = from === 'sheet';
        this.measure?.();
      }
      moved = true;
      this.dragging = true;
      this.progress = Math.min(1, Math.max(0, startProgress - dy / height));
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
      const open = flick ? velocity < 0 : this.progress > (from === 'bar' ? 0.25 : 0.75);
      this.settle(open ? 1 : 0);
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
