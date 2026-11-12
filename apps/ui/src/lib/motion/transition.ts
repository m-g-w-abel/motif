/**
 * The one model every animation follows (docs/ANIMATION.md): a transition
 * is a set of tracks, each a function of one progress value from 0 (before)
 * to 1 (after). A clock plays it (a tap) or a finger seeks it (a gesture),
 * and either way it can end at 1 or go back to 0 from wherever it is.
 */

/** How long a whole transition takes on the clock (ms). */
export const DURATION = 260;
/** With reduced motion: fades only, quicker. */
const STILL_DURATION = 120;

/** A cubic Bézier timing curve with its inverse, so progress and time convert both ways. */
function bezier(x1: number, y1: number, x2: number, y2: number) {
  const b = (a: number, c: number, s: number) => 3 * a * s * (1 - s) ** 2 + 3 * c * s * s * (1 - s) + s ** 3;
  /** The curve's parameter where `f` reaches `v` (f rises from 0 to 1). */
  const solve = (f: (s: number) => number, v: number) => {
    let lo = 0;
    let hi = 1;
    for (let i = 0; i < 40; i++) {
      const mid = (lo + hi) / 2;
      if (f(mid) < v) lo = mid;
      else hi = mid;
    }
    return (lo + hi) / 2;
  };
  const x = (s: number) => b(x1, x2, s);
  const y = (s: number) => b(y1, y2, s);
  const clamp = (v: number) => Math.min(1, Math.max(0, v));
  return {
    css: `cubic-bezier(${x1}, ${y1}, ${x2}, ${y2})`,
    /** Progress after `t` (0 to 1) of the time. */
    at: (t: number) => (t <= 0 ? 0 : t >= 1 ? 1 : y(solve(x, clamp(t)))),
    /** The time (0 to 1) at which progress reaches `p`. */
    inverse: (p: number) => (p <= 0 ? 0 : p >= 1 ? 1 : x(solve(y, clamp(p)))),
  };
}

export const CURVE = bezier(0.2, 0.8, 0.2, 1);

export type Span = [number, number];

/** One element's part: keyframes (transform and opacity only) over a span of progress (default all of it). */
export interface Track {
  el: HTMLElement;
  frames: Keyframe[];
  span?: Span;
}

/** What a gesture drives: progress as the finger goes, then whether it completes. */
export interface Drive {
  seek(p: number): void;
  release(commit: boolean): void;
}

export const still = () => typeof matchMedia !== 'undefined' && matchMedia('(prefers-reduced-motion: reduce)').matches;

/**
 * Tracks as paused Web Animations on one clock of `DURATION`, eased by
 * `CURVE`. A span in progress becomes a stretch of that clock through the
 * curve's inverse, so seeking to `p` draws exactly `p` of the way (linear
 * with the finger), and playing on decelerates along the curve.
 */
export class Transition implements Drive {
  /** Dev builds (`?dev-scrub=0.3`): the next transition stops there instead of playing. */
  static hold: number | null = null;

  readonly #animations: Animation[] = [];
  readonly #still = still();
  readonly #length = this.#still ? STILL_DURATION : DURATION;
  #cleanup?: () => void;
  #to: 0 | 1 | null = null;
  #done?: Promise<void>;
  #ended = false;
  #at = 0;

  constructor(tracks: Track[], cleanup?: () => void) {
    this.#cleanup = cleanup;
    for (const t of tracks) this.add(t);
  }

  /** How far it is drawn, 0 to 1. */
  get progress() {
    const a = this.#animations[0];
    if (!a || a.currentTime === null) return this.#at;
    return CURVE.at(Number(a.currentTime) / this.#length);
  }

  /** Where it is heading once released (null: not yet). */
  get heading() {
    return this.#to;
  }

  /** Joins a track (one whose element came late, a card loaded in) where the others are now. */
  add({ el, frames, span = [0, 1] }: Track) {
    const kept = this.#still ? frames.map(({ transform: _, ...rest }) => rest) : frames;
    const [a, b] = span.map((p) => CURVE.inverse(p) * this.#length);
    const animation = el.animate(kept, { delay: a, duration: Math.max(1, b - a), endDelay: this.#length - b, fill: 'both', easing: CURVE.css });
    animation.pause();
    animation.currentTime = this.#time();
    if (this.#to !== null && !this.#ended) {
      animation.playbackRate = this.#to ? 1 : -1;
      animation.play();
    }
    this.#animations.push(animation);
  }

  #time() {
    const a = this.#animations[0];
    return a && a.currentTime !== null ? Number(a.currentTime) : CURVE.inverse(this.#at) * this.#length;
  }

  /** Draws progress `p` (a finger). Pauses a transition that was playing, where it is. */
  seek(p: number) {
    if (this.#to !== null) return;
    this.#at = Math.min(1, Math.max(0, p));
    const time = CURVE.inverse(this.#at) * this.#length;
    for (const a of this.#animations) {
      if (a.playState !== 'paused') a.pause();
      a.currentTime = time;
    }
  }

  /** Plays on from where it is to `to`; resolves once, when it has got there and cleaned up. */
  play(to: 0 | 1): Promise<void> {
    if (this.#done) return this.#done;
    if (Transition.hold !== null) {
      const p = Transition.hold;
      Transition.hold = null;
      this.seek(p);
      // Frozen for inspection; the next change ends it.
      this.#done = new Promise(() => {});
      this.#to = to;
      return this.#done;
    }
    this.#to = to;
    for (const a of this.#animations) {
      a.playbackRate = to ? 1 : -1;
      a.play();
    }
    // Cancelled ones (their element gone) count as finished.
    this.#done = Promise.allSettled(this.#animations.map((a) => a.finished)).then(() => this.#finish(to));
    return this.#done;
  }

  /** A gesture let go: on to the end, or back to the start. */
  release(commit: boolean) {
    return this.play(commit ? 1 : 0);
  }

  /** Jumps to where it was heading (the end, if it wasn't released) and cleans up. */
  end() {
    this.#finish(this.#to ?? 1);
  }

  #finish(to: 0 | 1) {
    if (this.#ended) return;
    this.#ended = true;
    this.#to = to;
    this.#at = to;
    for (const a of this.#animations) a.cancel();
    this.#animations.length = 0;
    this.#cleanup?.();
  }
}
