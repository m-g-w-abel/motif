import { tick } from 'svelte';
import { fly, rootZoom, see, type Flight, type Seen } from './mover';
import { Transition, type Drive, type Track } from './transition';

/** Into a deeper screen, back out of one, or across to another at the same depth. */
export type Motion = 'forward' | 'back' | 'swap';

export function motion(from: number, to: number): Motion {
  return to > from ? 'forward' : to < from ? 'back' : 'swap';
}

/** A screen the stage shows: `key` names it (the same key is the same screen, never mounted twice). */
export interface Screen {
  key: string;
  depth: number;
}

/** How far the screen beneath moves while the one on top covers it. */
const UNDER = '-8%';
/** How long shared elements that haven't appeared yet (a grid's cards) are looked for. */
const WAIT = 500;

const frame = () => new Promise((r) => requestAnimationFrame(r));

/**
 * The screens of an area (docs/ANIMATION.md, Screens): a keyed list of at
 * most the screen showing and the one it is changing to or from, or, at
 * rest, the one it came forward from, kept hidden beneath (so going back
 * starts at once, with nothing to render). Forward, the new screen slides in
 * from the right over the old, which moves a little left; back is the same
 * played backwards, by a clock (`go`) or a finger (`drag`, with the screen
 * it goes back to beneath, which then simply stays). Across, the new one
 * fades in over the old. Shared elements (`data-move`) travel between them.
 */
export class Stage<S extends Screen> {
  shown = $state.raw<S[]>([]);
  #root: () => HTMLElement | undefined;
  #t: Transition | null = null;
  /** The change under way: the screen it ends on (a pending one until built), and whether a finger drives it. */
  #change: { to: S; from: S; drag: boolean } | null = null;
  #token = 0;

  constructor(first: S, root: () => HTMLElement | undefined) {
    this.shown = [first];
    this.#root = root;
  }

  get top() {
    return this.shown.at(-1)!;
  }

  #el(key: string) {
    return [...(this.#root()?.children ?? [])].find((e): e is HTMLElement => e instanceof HTMLElement && e.dataset.screen === key);
  }

  /** The state shows `next` now: animates there, unless already there or going there. */
  go(next: S) {
    if (next.key === (this.#change?.to.key ?? this.top.key)) return;
    this.#settle();
    const from = this.top;
    if (from.key === next.key) return;
    void this.#begin(from, next, motion(from.depth, next.depth), false).then((t) => t?.play(1));
  }

  /**
   * A swipe back to `under`: it is mounted beneath and the change follows
   * the finger. Releasing to commit calls `commit` (which changes the state
   * to `under`) at once and plays on from there; otherwise it springs back.
   */
  drag(under: S, commit: () => void): Drive {
    this.#settle();
    const from = this.top;
    let p = 0;
    let released: boolean | null = null;
    let t: Transition | null = null;
    // Moves with the finger from its first move, while the screen beneath is still being laid out
    // (a frame or two); the transition takes over from there.
    const fromEl = this.#el(from.key);
    const early = (q: number) => fromEl && (fromEl.style.transform = q ? `translateX(${q * 100}%)` : '');
    void this.#begin(from, under, 'back', true, () => early(0)).then((built) => {
      t = built;
      if (!t) return;
      t.seek(p);
      if (released !== null) void t.release(released);
    });
    return {
      seek: (q) => {
        p = q;
        if (t) t.seek(q);
        else if (released === null) early(q);
      },
      release: (yes) => {
        if (released !== null) return;
        released = yes;
        // Committed: ending it early jumps to the end, not back.
        if (yes && this.#change?.from === from) this.#change.drag = false;
        if (yes) commit();
        if (t) void t.release(yes);
        else if (!yes) {
          early(0);
          this.#settle();
        }
      },
    };
  }

  /** Ends whatever is under way at once: a finger's change springs back, a clock's jumps to its end. */
  #settle() {
    const c = this.#change;
    if (this.#t) {
      if (c?.drag) void this.#t.release(false);
      this.#t.end();
    } else if (c) {
      // Not built yet: show where it was going (a drag: where it came from).
      const end = c.drag ? c.from : c.to;
      this.shown = [end];
      const el = this.#el(end.key);
      // Unseen while it was laid out, or moved by a finger ahead of the transition.
      if (el) Object.assign(el.style, { visibility: '', transform: '' });
    }
    this.#token++;
    this.#t = null;
    this.#change = null;
  }

  async #begin(from: S, to: S, how: 'forward' | 'back' | 'swap', drag: boolean, ready?: () => void): Promise<Transition | null> {
    const token = ++this.#token;
    this.#change = { from, to, drag };
    // Back to the screen kept beneath: laid out already, so the change starts now.
    const kept = how === 'back' && this.shown.length === 2 && this.shown[0].key === to.key;
    this.shown = how === 'back' ? [to, from] : [from, to];
    if (!kept) await tick();
    const toEl = this.#el(to.key);
    const fromEl = this.#el(from.key);
    if (token !== this.#token) return null;
    if (!toEl || !fromEl) {
      this.shown = [to];
      this.#change = null;
      return null;
    }
    if (!kept) {
      // Laid out, unseen, for a frame: its cards, its scroll position.
      toEl.style.visibility = 'hidden';
      await frame();
      if (token !== this.#token) return null;
    }
    // Shown in the same task as its first frame is set (no paint between).
    ready?.();
    toEl.style.visibility = '';
    const t = this.#build(fromEl, toEl, how, (landed) => {
      if (token !== this.#token) return;
      // The shallower of the two stays beneath, hidden, when it is the one back goes to.
      const [end, other, otherEl] = landed ? [to, from, fromEl] : [from, to, toEl];
      const keep = other.depth < end.depth;
      if (keep) otherEl.style.visibility = 'hidden';
      this.shown = keep ? [other, end] : [end];
      this.#t = null;
      this.#change = null;
    });
    this.#t = t;
    return t;
  }

  #build(fromEl: HTMLElement, toEl: HTMLElement, how: Motion, ended: (landed: boolean) => void) {
    const slide = (el: HTMLElement, a: string, b: string): Track => ({ el, frames: [{ transform: a }, { transform: b }] });
    const tracks: Track[] =
      how === 'forward'
        ? [slide(toEl, 'translateX(100%)', 'none'), slide(fromEl, 'none', `translateX(${UNDER})`)]
        : how === 'back'
          ? [slide(fromEl, 'none', 'translateX(100%)'), slide(toEl, `translateX(${UNDER})`, 'none')]
          : [{ el: toEl, frames: [{ opacity: 0 }, { opacity: 1 }] }];
    // Opaque, so only what one uncovers of the other shows (never what lies under both, like the player bar).
    const kept = [fromEl, toEl].map((el) => el.style.cssText);
    // (Presses: the screen that isn't the state's is inert; pointer-events: none would cancel the finger driving it.)
    for (const el of [fromEl, toEl]) if (getComputedStyle(el).backgroundColor === 'rgba(0, 0, 0, 0)') el.style.background = 'var(--bg)';

    // Shared elements, measured at rest; those whose counterpart hasn't come yet are looked for as it goes.
    let waiting: Seen[] = [...fromEl.querySelectorAll<HTMLElement>('[data-move]')].filter((el) => el.offsetWidth).map((el) => see(el));
    const flights: Flight[] = [];
    let t: Transition | null = null;
    const pair = () => {
      waiting = waiting.filter((from) => {
        const to = toEl.querySelector<HTMLElement>(`[data-move="${CSS.escape(from.el.dataset.move!)}"]`);
        if (!to?.offsetWidth) return true;
        // Where it will rest: without the slide its screen is in.
        const box = to.getBoundingClientRect();
        // Its transform is in CSS pixels, the box on screen: the root's zoom between them.
        const shift = new DOMMatrix(getComputedStyle(toEl).transform).m41 * rootZoom();
        const f = fly(from, to, new DOMRect(box.x - shift, box.y, box.width, box.height));
        flights.push(f);
        if (t) for (const track of f.tracks) t.add(track);
        else tracks.push(...f.tracks);
        return false;
      });
    };
    pair();
    // Not on every pointer move (seeking only sets times): once a frame, while a finger holds it and a little after.
    let over = false;
    /** When it was released (or started playing). */
    let since: number | undefined;
    const look = () => {
      if (t && t.heading !== null) since ??= performance.now();
      if (over || !waiting.length || (since !== undefined && performance.now() - since > WAIT)) return;
      pair();
      requestAnimationFrame(look);
    };
    requestAnimationFrame(look);
    t = new Transition(tracks, () => {
      over = true;
      const landed = t!.heading === 1;
      for (const f of flights) f.done(landed);
      fromEl.style.cssText = kept[0];
      toEl.style.cssText = kept[1];
      ended(landed);
    });
    return t;
  }
}
