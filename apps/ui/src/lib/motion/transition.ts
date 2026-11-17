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

type Point = [number, number];
const lerp = (a: Point, b: Point, t: number): Point => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
/** The part of the cubic Bézier `p` from parameter `s0` to `s1` (de Casteljau), itself a cubic Bézier. */
function part(p: Point[], s0: number, s1: number): Point[] {
  const split = (q: Point[], t: number) => {
    const [a, b, c] = [lerp(q[0], q[1], t), lerp(q[1], q[2], t), lerp(q[2], q[3], t)];
    const [d, e] = [lerp(a, b, t), lerp(b, c, t)];
    const f = lerp(d, e, t);
    return { left: [q[0], a, d, f], right: [f, e, c, q[3]] };
  };
  const left = split(p, s1).left;
  return s1 > 0 ? split(left, s0 / s1).right : left;
}

/** A cubic Bézier timing curve with its inverse, so progress and time convert both ways. */
function bezier(x1: number, y1: number, x2: number, y2: number): Curve {
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
    /**
     * The curve between progress `a` and `b`, stretched to fill its own box:
     * a track over that span with this easing is linear in progress, so a
     * finger draws it exactly and it plays the same either way.
     */
    segment(a: number, b: number): Curve {
      if (a <= 0 && b >= 1) return this;
      const [s0, s1] = [a <= 0 ? 0 : solve(y, a), b >= 1 ? 1 : solve(y, b)];
      const q = part(
        [
          [0, 0],
          [x1, y1],
          [x2, y2],
          [1, 1],
        ],
        s0,
        s1,
      );
      const [w, h] = [q[3][0] - q[0][0], q[3][1] - q[0][1]];
      const n = (k: number): Point => [(q[k][0] - q[0][0]) / w, (q[k][1] - q[0][1]) / h];
      const [c1, c2] = [n(1), n(2)];
      return bezier(c1[0], c1[1], c2[0], c2[1]);
    },
  };
}

interface Curve {
  css: string;
  at(t: number): number;
  inverse(p: number): number;
  segment(a: number, b: number): Curve;
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
 * Tracks as paused Web Animations on one clock of `DURATION`. A span in
 * progress becomes a stretch of that clock through the curve's inverse, and
 * its easing is that stretch of the curve (`segment`), so every track is
 * linear in progress: seeking to `p` draws exactly `p` of the way (linear
 * with the finger). A clock eases out the way it travels: forwards it plays
 * the tracks on; backwards (closing, a tap back, a drag springing back) it
 * plays the mirrored tracks (frames reversed, spans mirrored) on from the
 * mirrored progress, which draw the same path, so it never eases in.
 */
export class Transition implements Drive {
  /** Dev builds (`?dev-scrub=0.3`): the next transition stops there instead of playing. */
  static hold: number | null = null;

  readonly #tracks: Track[] = [];
  #animations: Animation[] = [];
  /** Drawn by the mirrored tracks: their progress is 1 less ours. */
  #mirrored = false;
  readonly #still = still();
  readonly #length = this.#still ? STILL_DURATION : DURATION;
  #cleanup?: () => void;
  #to: 0 | 1 | null = null;
  #ended = false;
  #over!: () => void;
  /** Resolves once, when it has ended and cleaned up. */
  readonly done = new Promise<void>((r) => (this.#over = r));
  /** Counts plays, so one a finger caught never ends it. */
  #play = 0;
  #at = 0;

  constructor(tracks: Track[], cleanup?: () => void) {
    this.#cleanup = cleanup;
    for (const t of tracks) this.add(t);
  }

  /** How far it is drawn, 0 to 1. */
  get progress() {
    const a = this.#animations[0];
    if (!a || a.currentTime === null) return this.#at;
    const q = CURVE.at(Number(a.currentTime) / this.#length);
    return this.#mirrored ? 1 - q : q;
  }

  /** Where it is heading once released (null: not yet). */
  get heading() {
    return this.#to;
  }

  /** The clock's time that draws progress `p`. */
  #time(p: number) {
    return CURVE.inverse(this.#mirrored ? 1 - p : p) * this.#length;
  }

  /** One track as an animation the way it is drawn now (mirrored or not), at the time that draws `p`. */
  #animate({ el, frames, span = [0, 1] }: Track, p: number) {
    const kept = this.#still ? frames.map(({ transform: _, ...rest }) => rest) : frames;
    const [from, to] = this.#mirrored ? [1 - span[1], 1 - span[0]] : span;
    const keys = this.#mirrored ? [...kept].reverse().map((k) => (k.offset == null ? k : { ...k, offset: 1 - Number(k.offset) })) : kept;
    const [a, b] = [from, to].map((q) => CURVE.inverse(q) * this.#length);
    const animation = el.animate(keys, { delay: a, duration: Math.max(1, b - a), endDelay: this.#length - b, fill: 'both', easing: CURVE.segment(from, to).css });
    animation.pause();
    animation.currentTime = this.#time(p);
    return animation;
  }

  /** Joins a track (one whose element came late, a card loaded in) where the others are now. */
  add(track: Track) {
    const p = this.progress;
    this.#tracks.push(track);
    const animation = this.#animate(track, p);
    if (this.#to !== null && !this.#ended) animation.play();
    this.#animations.push(animation);
  }

  /** Draws progress `p` (a finger). Pauses a transition that was playing, where it is. */
  seek(p: number) {
    if (this.#to !== null) return;
    this.#at = Math.min(1, Math.max(0, p));
    const time = this.#time(this.#at);
    for (const a of this.#animations) {
      if (a.playState !== 'paused') a.pause();
      a.currentTime = time;
    }
  }

  /** Plays on from where it is to `to`; resolves once, when it has got there and cleaned up. */
  play(to: 0 | 1): Promise<void> {
    if (this.#ended || this.#to !== null) return this.done;
    this.#to = to;
    if (Transition.hold !== null) {
      const p = Transition.hold;
      Transition.hold = null;
      this.#to = null;
      this.seek(p);
      // Frozen for inspection; the next change ends it.
      this.#to = to;
      return this.done;
    }
    const play = ++this.#play;
    const finish = () => play === this.#play && this.#finish(to);
    // Already there (a swipe let go at the end): playing would rewind it and run it again.
    const p = this.progress;
    if (Math.abs(p - to) < 0.5 / this.#length) {
      void Promise.resolve().then(finish);
      return this.done;
    }
    // The way it travels decides which tracks draw it, so the clock eases out towards `to`.
    if (this.#mirrored !== (to === 0)) {
      for (const a of this.#animations) a.cancel();
      this.#mirrored = to === 0;
      this.#animations = this.#tracks.map((t) => this.#animate(t, p));
    }
    for (const a of this.#animations) a.play();
    // Cancelled ones (their element gone) count as finished.
    void Promise.allSettled(this.#animations.map((a) => a.finished)).then(finish);
    return this.done;
  }

  /**
   * A finger catches it while it plays: it stops where it is drawn, and
   * seeking and letting go work again from there. Its progress, or null
   * once it has ended.
   */
  grab(): number | null {
    if (this.#ended) return null;
    this.#at = this.progress;
    for (const a of this.#animations) a.pause();
    this.#to = null;
    this.#play++;
    return this.#at;
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
    this.#over();
  }
}

/** How long a transition that took over behind the finger takes to catch up with it (ms). */
export const CATCH = 120;

/**
 * What a finger drives (docs/ANIMATION.md, Following the finger): every
 * gesture goes through this, so none jumps. The transition may still be
 * being built (laid out, waiting for data, measured) or may have held the
 * main thread while the finger moved on. Meanwhile `early` draws what it
 * can, exactly with the finger; the transition then takes over where that
 * left the drawing (else at `from`) on its first frame and catches up with
 * the finger over `CATCH` ms, after which it follows it exactly. Letting go
 * plays on from what is drawn, built or not.
 */
export function follow(build: Drive | null | Promise<Drive | null>, { early, from = 0 }: { early?: (p: number) => void; from?: number } = {}): Drive {
  let t: Drive | null = null;
  /** The finger's progress, and what is drawn. */
  let q = from;
  let shown = from;
  let released: boolean | null = null;
  /** Catching up: the gap between the drawing and the finger on the transition's first frame, and when that was. */
  let catching: { gap: number; since: number } | null = null;
  let live = false;
  const draw = (p: number) => {
    shown = p;
    t!.seek(p);
  };
  const step = (now: number) => {
    if (released !== null) return;
    catching ??= { gap: shown - q, since: now };
    const k = 1 - (now - catching.since) / CATCH;
    if (k <= 0) {
      catching = null;
      return draw(q);
    }
    draw(q + catching.gap * k * k);
    requestAnimationFrame(step);
  };
  const take = (built: Drive | null) => {
    t = built;
    if (!t) return;
    t.seek(shown);
    if (released !== null) return void t.release(released);
    requestAnimationFrame((now) => {
      live = true;
      step(now);
    });
  };
  if (build instanceof Promise) void build.then(take);
  else take(build);
  return {
    seek(p) {
      if (released !== null) return;
      q = p;
      if (!t) {
        if (!early) return;
        early(p);
        shown = p;
      } else if (live && !catching) draw(p);
    },
    release(commit) {
      if (released !== null) return;
      released = commit;
      if (t) void t.release(commit);
    },
  };
}
