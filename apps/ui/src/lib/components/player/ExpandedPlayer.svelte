<script lang="ts">
  import { flushSync, tick, untrack } from 'svelte';
  import { SLOP } from '../../gestures';
  import { Scrub } from '../../screens';
  import { released, stepTo, type SheetStep } from '../../sheet.svelte';
  import { getApp, setViewState } from '../../context';
  import { mapped } from '../../gestures';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';
  import Transport from './Transport.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ArtworkWidget from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Lyrics from '../widgets/Lyrics.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';
  import { trackTitle } from '../../format';
  import { marquee } from '../../marquee';

  const { engine, layout, settings, sheet, views } = getApp();
  let body = $state<HTMLElement>();
  let section = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  let tint = $state<HTMLElement>();
  setViewState({ source: 'library', layoutId: 'now-playing', browse: layout.browseState('now-playing') });

  /**
   * The morph runs in stages so no two moving things cross: the bar's own
   * controls fade out where they are (OUT), the artwork, title and artist
   * travel together (MOVE), then the sheet's controls rise into place (IN).
   * Bar element → its counterpart here, the box to match (`box` stretches,
   * `even` scales evenly, `line` stretches across only, `text` matches the
   * first letter), and what it does:
   * - `move`: the sheet's own element travels; nothing to hand over.
   * - `cross`: a copy of the bar's (a ghost) travels with the sheet's and they
   *   crossfade once both have arrived, drawn at the same place and size.
   * - `handover`: like `cross`, but the contents differ (the bar's line has
   *   the album too), so the ghost is gone before the sheet's appears.
   * - `stage`: the bar's fades out in OUT, the sheet's rises in in IN.
   * - `edge`: the bar's strip rides the top edge of the growing background
   *   and fades; the sheet's seek bar rises in in IN. On phones, where the
   *   strip sits under the track, it travels to the seek bar instead (`cross`).
   * Each can appear twice, for the counterpart shown here (phone or desktop).
   */
  const MORPH: [string, string, Mode, Kind][] = [
    ['.art', '.frame', 'box', 'move'],
    ['.label .title', '.info h1', 'text', 'cross'],
    ['.label .artist', '.info .hero-artist', 'text', 'cross'],
    ['.label .album', '.album-title', 'text', 'cross'],
    ['.label .album', '.info .album-name', 'text', 'cross'],
    ['.seek .slider', '.controls .seek .slider', 'line', 'edge'],
    ['.transport [aria-label="Previous"]', '.controls [aria-label="Previous"]', 'box', 'stage'],
    ['.transport .play', '.controls .transport .play', 'even', 'cross'],
    ['.transport [aria-label="Next"]', '.controls [aria-label="Next"]', 'box', 'stage'],
    ['.time', '.controls .seek .time:first-child', 'box', 'stage'],
    ['.total', '.controls .seek .time:last-child', 'box', 'stage'],
    ['[aria-label^="Repeat"]', '.controls [aria-label^="Repeat"]', 'box', 'stage'],
    ['[aria-label="Shuffle"]', '.controls [aria-label="Shuffle"]', 'box', 'stage'],
    ['.volume', '.controls .volume', 'box', 'stage'],
    ['.end > [aria-label="Expand player"]', 'header [aria-label="Collapse"]', 'box', 'stage'],
  ];
  /** No counterpart in the bar: rise in in RISE (the carousel's side cards and controls after its cover). */
  const FADE =
    'header .handle, header .lyrics-toggle, .art.lyrics, .hero-album .format, .lyrics-button, .more, .coverflow .card:not(.frame), .coverflow .turns, .coverflow .below';
  /** In the bar only: the album's separator fades as its neighbours leave, and the view buttons fall away under the screen. */
  const LEAVE: [string, boolean][] = [
    ['.label .sep', false],
    ['.buttons', true],
  ];
  /** How far parts with no counterpart rise from (px). */
  const RISE_BY = 40;

  type Mode = 'box' | 'even' | 'line' | 'text';
  type Kind = 'move' | 'cross' | 'handover' | 'stage' | 'edge';
  /** A stretch of progress. */
  type Span = [number, number];
  /** The background grows until the movers arrive, so they never poke out of it. */
  const GROW: Span = [0, 0.85];
  const OUT: Span = [0, 0.1];
  const MOVE: Span = [0.1, 0.85];
  const IN: Span = [0.85, 1];
  /** Text under the artwork (phones) waits until the growing artwork has risen past it. */
  const MOVE_UNDER: Span = [0.4, 0.85];
  const ALBUM_MOVE: Span = [0.2, 0.85];
  /** Parts with no counterpart rise into place once the movers are nearly there. */
  const RISE: Span = [0.6, 1];
  /** The view buttons fall away while the background grows over the bar. */
  const FALL: Span = [0, 0.5];
  const DURATION = 260;
  const CURVE = 'cubic-bezier(0.2, 0.8, 0.2, 1)';

  /**
   * `el` is moved over `move` by (dx, dy, sx, sy): the sheet's own parts start
   * at the bar and arrive at rest, ghosts start at rest (over the bar) and
   * arrive at the sheet. It fades in over `show` or out over `hide`.
   */
  type Part = { el: HTMLElement; ghost: boolean; dx: number; dy: number; sx: number; sy: number; move?: Span; show?: Span; hide?: Span };
  const STILL = { dx: 0, dy: 0, sx: 1, sy: 1 };
  // Plain, not state: measured once per gesture, read by the effect below.
  let parts: Part[] = [];
  /** The progress last drawn, where a settle's transitions start from. */
  let drawn = 0;
  /** The background's corner radius at the bar (px) and its scale there, on phones. */
  let corner: { r: number; sx: number; sy: number } | null = null;
  /**
   * GROW and MOVE (or MOVE_UNDER for text), set per measure. A drag down
   * from the sheet (`sheet.scrub`) runs both over the whole drag instead, so
   * the background and the movers follow the finger from its first pixel.
   */
  let growing = GROW;
  let moving = MOVE;
  let textMove = MOVE;
  let ghosts = $state<HTMLElement>();

  function shown(root: Element, selector: string) {
    return [...root.querySelectorAll<HTMLElement>(selector)].find((e) => e.offsetWidth > 0);
  }

  /** The box of the first letter or digit, or of the element without one. */
  function glyph(el: HTMLElement): DOMRect {
    const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walk.nextNode(); n; n = walk.nextNode()) {
      const i = n.textContent?.search(/[\p{L}\p{N}]/u) ?? -1;
      if (i < 0) continue;
      const range = document.createRange();
      range.setStart(n, i);
      range.setEnd(n, i + 1);
      return range.getBoundingClientRect();
    }
    return el.getBoundingClientRect();
  }

  /** The box of all of the element's text. */
  function ink(el: HTMLElement): DOMRect {
    const range = document.createRange();
    range.selectNodeContents(el);
    return range.getBoundingClientRect();
  }

  /** FLIP: the transform (origin top left) that puts box `x`, inside `e`, over box `y`. */
  function fit(e: DOMRect, x: DOMRect, y: DOMRect, mode: Mode) {
    let sx = y.width / x.width;
    let sy = y.height / x.height;
    if (mode === 'text') sx = sy;
    else if (mode === 'even') sx = sy = Math.min(sx, sy);
    else if (mode === 'line') sy = 1;
    const dx = y.left + y.width / 2 - e.left - (x.left + x.width / 2 - e.left) * sx;
    const dy = y.top + y.height / 2 - e.top - (x.top + x.height / 2 - e.top) * sy;
    return { dx, dy, sx, sy };
  }

  /** A copy of the sheet's own `b` in `colour`, laid exactly over it. */
  function tinted(b: HTMLElement, colour: string, at: DOMRect) {
    const r = b.getBoundingClientRect();
    const look = getComputedStyle(b);
    const el = b.cloneNode(true) as HTMLElement;
    for (const p of ['font-family', 'font-size', 'font-weight', 'line-height', 'letter-spacing', 'white-space', 'text-align', 'display', 'justify-content', 'overflow'])
      el.style.setProperty(p, look.getPropertyValue(p));
    Object.assign(el.style, {
      position: 'absolute',
      left: `${r.left - at.left}px`,
      top: `${r.top - at.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
      margin: '0',
      color: colour,
      transformOrigin: '0 0',
    });
    for (const n of el.querySelectorAll<HTMLElement>('*')) n.style.color = 'inherit';
    ghosts?.append(el);
    return el;
  }

  /** A copy of the bar's `a`, laid over it here. */
  function copy(a: HTMLElement, selector: string, at: DOMRect) {
    const r = a.getBoundingClientRect();
    const el = a.cloneNode(true) as HTMLElement;
    // Nested parts (the total inside the bar's time) are copied on their own.
    for (const [from] of MORPH) if (from !== selector) for (const n of el.querySelectorAll(from)) n.remove();
    Object.assign(el.style, {
      position: 'absolute',
      left: `${r.left - at.left}px`,
      top: `${r.top - at.top}px`,
      width: `${r.width}px`,
      height: `${r.height}px`,
      margin: '0',
      transformOrigin: '0 0',
    });
    ghosts?.append(el);
    return el;
  }

  /** A part with no counterpart (or one that hands over): it rises into place and fades in. */
  const rising = (el: HTMLElement): Part => ({ el, ghost: false, ...STILL, dy: RISE_BY, move: RISE, show: RISE });

  /** The parts that carry bar element `a` to sheet element `b`. */
  function pair(a: HTMLElement, from: string, b: HTMLElement, mode: Mode, kind: Kind, at: DOMRect, o: DOMRect): Part[] {
    const ra = a.getBoundingClientRect();
    const rb = b.getBoundingClientRect();
    const [ga, gb] = mode === 'text' ? [glyph(a), glyph(b)] : [ra, rb];
    b.style.transformOrigin = '0 0';
    // The album's line starts under the title in the bar and ends above it: it sets off first, to be past it before the title leaves.
    const move = mode !== 'text' ? moving : b.matches('.album-title') && textMove === MOVE_UNDER ? ALBUM_MOVE : textMove;
    if (kind === 'cross' && mode === 'text') {
      const [ia, ib] = [ink(a), ink(b)];
      // Same text on one line in both: the sheet's travels alone, matched by width, since any
      // crossfade between two drawings of it shows (#109). Cut-off or wrapped titles still crossfade.
      if (a.scrollWidth <= a.clientWidth && ib.height < 1.5 * gb.height) {
        const sx = ia.width / ib.width;
        const sy = ga.height / gb.height;
        const dx = ia.left - rb.left - (ib.left - rb.left) * sx;
        const dy = ga.top + ga.height / 2 - rb.top - (gb.top + gb.height / 2 - rb.top) * sy;
        const travel = { ghost: false, dx, dy, sx, sy, move };
        // In another colour (the bar's artist is muted, the sheet's coloured): a copy of the same
        // words in the bar's colour lies on it and fades as they go, so the colour changes, not the drawing.
        const from = getComputedStyle(a).color;
        if (from === getComputedStyle(b).color) return [{ el: b, ...travel }];
        return [{ el: b, ...travel }, { el: tinted(b, from, at), ...travel, hide: move }];
      }
    }
    const there = { el: b, ghost: false, ...fit(rb, gb, ga, mode), move };
    if (kind === 'move') return [there];
    if (kind === 'stage') return [rising(b), { el: copy(a, from, at), ghost: true, ...STILL, hide: OUT }];
    if (kind === 'edge') {
      // Keeps its place relative to the background (stretching only across).
      const sx = at.width / o.width;
      const dx = at.left + (ra.left - o.left) * sx - ra.left;
      const dy = at.top + (ra.top - o.top) * (at.height / o.height) - ra.top;
      return [rising(b), { el: copy(a, from, at), ghost: true, dx, dy, sx, sy: 1, move: growing, hide: [0.3, 0.5] }];
    }
    const ghost = { el: copy(a, from, at), ghost: true, ...fit(ra, ga, gb, mode), move };
    return [
      { ...there, show: IN },
      { ...ghost, hide: kind === 'cross' ? IN : [0.75, 0.85] },
    ];
  }

  /**
   * Measures where each part starts (in the bar or floating player) and ends
   * (here, untransformed), then pins them at the current progress without
   * easing, so the next change animates from there. Without a player to grow
   * out of, or with reduced motion, the whole sheet fades instead.
   */
  function measure() {
    if (!section || !backdrop || !ghosts) return;
    for (const { el } of parts) {
      el.style.transition = 'none';
      el.style.transform = '';
      el.style.opacity = '';
    }
    ghosts.replaceChildren();
    parts = [];
    const origin = sheet.origin;
    if (origin && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const at = section.getBoundingClientRect();
      const o = origin.getBoundingClientRect();
      const bd = backdrop.getBoundingClientRect();
      const art = section.querySelector('.frame')?.getBoundingClientRect();
      const info = section.querySelector('.info')?.getBoundingClientRect();
      const whole: Span = [0, 1];
      growing = sheet.scrub ? whole : GROW;
      moving = sheet.scrub ? whole : MOVE;
      textMove = sheet.scrub ? whole : art && info && info.top >= art.bottom ? MOVE_UNDER : MOVE;
      const grow = fit(bd, bd, o, 'box');
      const found: Part[] = [{ el: backdrop, ghost: false, ...grow, move: growing }];
      // Phones: the bar is a rounded card inset from the edges, whose corners open out to the screen's.
      const r = views.phone ? parseFloat(getComputedStyle(origin).borderTopLeftRadius) || 0 : 0;
      corner = r ? { r, sx: grow.sx, sy: grow.sy } : null;
      for (const [from, to, mode, k] of MORPH) {
        const b = shown(section, to);
        if (!b) continue;
        const a = shown(origin, from);
        const kind = k === 'edge' && views.phone ? 'cross' : k;
        found.push(...(a ? pair(a, from, b, mode, kind, at, o) : [rising(b)]));
      }
      for (const el of section.querySelectorAll<HTMLElement>(FADE)) if (el.offsetWidth) found.push(rising(el));
      for (const [from, falls] of LEAVE) {
        const a = shown(origin, from);
        if (a) found.push({ el: copy(a, from, at), ghost: true, ...STILL, dy: falls ? at.bottom - a.getBoundingClientRect().top : 0, move: FALL, hide: falls ? FALL : OUT });
      }
      // The queue rises from below the page once the artwork and seek strip are clear of it.
      const queue = section.querySelector<HTMLElement>('.queue-slot');
      if (queue) found.push({ el: queue, ghost: false, ...STILL, dy: at.bottom - queue.getBoundingClientRect().top, move: [0.5, 1] });
      parts = found;
    }
    apply(sheet.progress, true);
    // Commit the start styles before the transition to the new progress.
    void getComputedStyle(backdrop).transform;
  }

  function ramp(p: number, [a, b]: Span) {
    return Math.min(1, Math.max(0, (p - a) / (b - a)));
  }

  /**
   * Duration and delay that put span `s` where it falls in a settle from
   * `from` to `to`, as if progress moved evenly over the settle.
   */
  function timing([a, b]: Span, from: number, to: number) {
    const lo = Math.max(a, Math.min(from, to));
    const hi = Math.min(b, Math.max(from, to));
    const span = Math.abs(to - from);
    if (hi <= lo || !span) return '0s';
    const ms = (v: number) => `${Math.round((v / span) * DURATION)}ms`;
    return `${ms(hi - lo)} ${to > from ? ms(lo - from) : ms(from - hi)}`;
  }

  function apply(p: number, instant: boolean) {
    if (!section || !tint) return;
    const from = drawn;
    drawn = p;
    for (const { el, ghost, dx, dy, sx, sy, move, show, hide } of parts) {
      const fade = show ?? hide;
      if (instant) el.style.transition = 'none';
      else {
        // Duration first, then the curve, then the delay.
        const time = (s: Span, curve: string) => timing(s, from, p).replace(' ', ` ${curve} `);
        el.style.transition = [move && `transform ${time(move, CURVE)}`, fade && `opacity ${time(fade, 'linear')}`]
          .filter(Boolean)
          .join(', ');
      }
      if (move) {
        const t = ghost ? ramp(p, move) : 1 - ramp(p, move);
        el.style.transform = t ? `translate(${dx * t}px, ${dy * t}px) scale(${1 + (sx - 1) * t}, ${1 + (sy - 1) * t})` : '';
      }
      if (show) el.style.opacity = String(ramp(p, show));
      else if (hide) el.style.opacity = String(1 - ramp(p, hide));
    }
    if (backdrop) {
      // A paint, not a transform, but of one layer for a quarter second, and only on phones.
      const t = corner ? 1 - ramp(p, growing) : 0;
      backdrop.style.transition = instant ? 'none' : `border-radius ${timing(growing, from, p).replace(' ', ` ${CURVE} `)}, ${backdrop.style.transition}`;
      backdrop.style.borderRadius = corner && t ? `${(corner.r * t) / corner.sx}px / ${(corner.r * t) / corner.sy}px` : '';
    }
    // The whole sheet fades only without a morph; pinned at once, so a first morph after the fade's
    // 0 doesn't fade the sheet in over the bar as well (the bar showed through it, doubled).
    section.style.transition = instant ? 'none' : '';
    section.style.opacity = parts.length ? '' : String(p);
    tint.style.transition = instant ? 'none' : `opacity ${timing(growing, from, p)}`;
    tint.style.opacity = parts.length ? String(1 - ramp(p, growing)) : '0';
  }

  /** The parts a phone's step (now playing, half or full queue, lyrics) shows or hides. */
  const STEPPED = ['.mini', '.now > .art:not(.lyrics)', '.now > .art.lyrics', '.cover-bg', '.details', '.queue-slot'];
  /**
   * Parts shown on more than one step, which travel between them: where each
   * is on each step, and how it scales. Listed outside in: one inside
   * another (the play button in the controls) travels relative to it.
   */
  const ROLES: [string, string[], Mode][] = [
    ['cover', ['.now > .art:not(.lyrics) .frame', '.cover-bg', '.mini .art'], 'even'],
    ['controls', ['.details > .controls'], 'box'],
    ['title', ['.info h1', '.mini-names b'], 'text'],
    ['artist', ['.info .hero-artist', '.mini-names span'], 'text'],
    ['play', ['.details .controls .play', '.mini .transport .play'], 'even'],
  ];
  /** Parts of the details some steps leave out (the half queue): they fade where they are. */
  const FADING = ['.album-title', '.details .hero-album', '.more'];
  type Seen = { el: HTMLElement; box: DOMRect; copy: HTMLElement; display: string; frame?: DOMRect };
  /** A role's element, its box and the box to match (its words, for text). */
  type Role = { el: HTMLElement; box: DOMRect; inner: DOMRect };
  type Faded = { el: HTMLElement; box: DOMRect };
  type Step = { parts: Map<string, Seen>; roles: Map<string, Role>; faded: Map<string, Faded> };

  const showing = (el: HTMLElement | null | undefined): el is HTMLElement => !!el?.offsetWidth && getComputedStyle(el).visibility !== 'hidden';
  /** The sheet's own `selector`, not a copy standing in for it. */
  const own = (root: HTMLElement, selector: string) =>
    [...root.querySelectorAll<HTMLElement>(selector)].find((el) => !el.closest('.sinking, .ghosts')) ?? null;

  /** What the sheet shows now: its parts (with copies to stand in if they go), where each role is, and the details' fading parts. */
  function seen(root: HTMLElement, copies: boolean): Step {
    const roles = new Map<string, Role>();
    for (const [name, where, mode] of ROLES) {
      const el = where.map((w) => own(root, w)).find(showing);
      if (!el) continue;
      // Marked, so a copy of its part can leave it out (`stepChange`).
      el.dataset.role = name;
      const box = el.getBoundingClientRect();
      roles.set(name, { el, box, inner: mode === 'text' ? ink(el) : box });
    }
    const faded = new Map<string, Faded>();
    for (const selector of FADING) {
      const el = own(root, selector);
      if (showing(el)) faded.set(selector, { el, box: el.getBoundingClientRect() });
    }
    const parts = new Map<string, Seen>();
    for (const selector of STEPPED) {
      const el = own(root, selector);
      if (!showing(el)) continue;
      const frame = el.querySelector('.frame')?.getBoundingClientRect();
      const copy = copies ? (el.cloneNode(true) as HTMLElement) : el;
      parts.set(selector, { el, box: el.getBoundingClientRect(), copy, display: getComputedStyle(el).display, frame });
    }
    return { parts, roles, faded };
  }

  /** The loaded image's width inside `el` (0: none yet). */
  const resolution = (el: HTMLElement) => {
    const img = el.querySelector('img');
    return img?.complete ? img.naturalWidth : 0;
  };

  /** A copy of `el` laid at `box` in the sheet's ghost layer, its image drawn at once. */
  function ghostOf(el: HTMLElement, box: DOMRect, at: DOMRect) {
    const g = el.cloneNode(true) as HTMLElement;
    for (const img of g.querySelectorAll('img')) Object.assign(img, { loading: 'eager', decoding: 'sync' });
    Object.assign(g.style, {
      position: 'absolute',
      left: `${box.left - at.left}px`,
      top: `${box.top - at.top}px`,
      width: `${box.width}px`,
      height: `${box.height}px`,
      margin: '0',
      transformOrigin: '0 0',
      visibility: 'visible',
    });
    ghosts?.append(g);
    return g;
  }

  /**
   * Phones: a step change, as Web Animations (`easing` linear when a drag
   * scrubs them). Parts on both sides move to their new places; the cover
   * and the half queue's cover background trade places; parts that appear
   * rise from below the screen (riding the part under them, if that moves,
   * and fading in over what they cross), and parts that go sink away (or
   * ride) as fading copies behind the rest, as the layout drops them at once.
   * The cover, controls, title, artist and play button travel from their
   * old places to their new ones (leaving their copies); the cover as a copy
   * of whichever drawing is sharper, already loaded. The details' parts a
   * step leaves out fade where they are. Stepping back mirrors it.
   * `distance` is how far the change moves things (px).
   */
  function stepChange(root: HTMLElement, was: Step, easing: string) {
    const before = was.parts;
    const { parts: after, roles, faded } = seen(root, false);
    const at = root.getBoundingClientRect();
    const timing: KeyframeAnimationOptions = { duration: DURATION, easing, fill: 'both' };
    const animations: Animation[] = [];
    /** Each moving thing's starting offset on screen, which what is inside it takes off its own. */
    const lifts = new Map<HTMLElement, number>();
    const liftOf = (el: HTMLElement) => {
      let best: HTMLElement | undefined;
      for (const part of lifts.keys()) if (part !== el && part.contains(el) && (!best || best.contains(part))) best = part;
      return best ? lifts.get(best)! : 0;
    };
    const copies: HTMLElement[] = [];
    const shown: HTMLElement[] = [];
    let distance = 0;
    const go = (el: HTMLElement, transform: string, lift: number, fade = false) => {
      lifts.set(el, lift);
      distance = Math.max(distance, Math.abs(lift));
      el.style.transformOrigin = '0 0';
      animations.push(el.animate([{ transform, ...(fade && { opacity: 0 }) }, { transform: 'none', ...(fade && { opacity: 1 }) }], timing));
    };
    const dy = (k: string) => after.get(k)!.box.top - before.get(k)!.box.top;
    const kept = [...after.keys()].filter((k) => before.has(k) && after.get(k)!.el === before.get(k)!.el);
    const fresh = [...after.keys()].filter((k) => !kept.includes(k));
    const gone = [...before.keys()].filter((k) => !kept.includes(k));
    /** A copy standing in for a part that went, fading (and moving by `to`) behind the rest. */
    const leaving = (k: string, to: number) => {
      const { copy, box, display } = before.get(k)!;
      Object.assign(copy.style, {
        position: 'absolute',
        left: `${box.left - at.left}px`,
        top: `${box.top - at.top}px`,
        width: `${box.width}px`,
        height: `${box.height}px`,
        margin: '0',
        // The new step's rules would hide it.
        display,
        visibility: 'visible',
      });
      sinking?.append(copy);
      copies.push(copy);
      distance = Math.max(distance, Math.abs(to));
      animations.push(copy.animate([{ transform: 'none', opacity: 1 }, { transform: `translateY(${to}px)`, opacity: 0 }], timing));
      return copy;
    };
    // The cover and the half queue's background stand for each other; the one going fades where it was.
    const cover = '.now > .art:not(.lyrics)';
    for (const [x, y] of [['.cover-bg', cover], [cover, '.cover-bg']]) {
      const b = after.get(x);
      const a = before.get(y);
      if (!fresh.includes(x) || !gone.includes(y) || !a || !b) continue;
      const m = fit(b.box, b.frame ?? b.box, a.frame ?? a.box, 'even');
      go(b.el, `translate(${m.dx}px, ${m.dy}px) scale(${m.sx})`, m.dy);
      const shade = b.el.querySelector<HTMLElement>('.shade');
      if (shade) animations.push(shade.animate([{ opacity: 0 }, { opacity: 1 }], timing));
      if (y === '.cover-bg') leaving(y, 0);
      fresh.splice(fresh.indexOf(x), 1);
      gone.splice(gone.indexOf(y), 1);
    }
    for (const k of kept) if (Math.abs(dy(k)) >= 1) go(after.get(k)!.el, `translateY(${-dy(k)}px)`, -dy(k));
    /** The kept part right under a box, which it rides along with. */
    const under = (box: DOMRect, side: Map<string, Seen>) =>
      kept.filter((k) => side.get(k)!.box.top >= box.bottom - 1).sort((a, b) => side.get(a)!.box.top - side.get(b)!.box.top)[0];
    const rest = (keys: string[], side: Map<string, Seen>) => Math.min(...keys.filter((k) => !under(side.get(k)!.box, side)).map((k) => side.get(k)!.box.top));
    const rise = at.bottom - rest(fresh, after);
    for (const k of fresh) {
      const ride = under(after.get(k)!.box, after);
      if (ride) go(after.get(k)!.el, `translateY(${-dy(ride)}px)`, -dy(ride), true);
      else go(after.get(k)!.el, `translateY(${rise}px)`, rise);
    }
    // Shared parts: each from its old place, less what the thing it is in already carries.
    const travelled = new Set<string>();
    for (const [name, , mode] of ROLES) {
      const a = was.roles.get(name);
      const b = roles.get(name);
      if (!a || !b || (lifts.has(b.el) && b.el !== a.el)) continue;
      const lift = liftOf(b.el);
      const m = fit(b.box, b.inner, a.inner, mode);
      if (Math.abs(m.dy - lift) < 1 && Math.abs(m.dx) < 1 && Math.abs(m.sx - 1) < 0.01) continue;
      if (b.el !== a.el) travelled.add(name);
      // The cover as a copy of the sharper drawing already loaded (the strip's is small), the real one hidden till it lands.
      if (name === 'cover' && b.el !== a.el && resolution(a.el) >= resolution(b.el) && resolution(a.el)) {
        const g = ghostOf(a.el, a.box, at);
        const n = fit(a.box, a.box, b.box, mode);
        b.el.style.visibility = 'hidden';
        shown.push(b.el);
        copies.push(g);
        animations.push(g.animate([{ transform: 'none' }, { transform: `translate(${n.dx}px, ${n.dy}px) scale(${n.sx}, ${n.sy})` }], timing));
        continue;
      }
      lifts.set(b.el, m.dy);
      b.el.style.transformOrigin = '0 0';
      animations.push(b.el.animate([{ transform: `translate(${m.dx}px, ${m.dy - lift}px) scale(${m.sx}, ${m.sy})` }, { transform: 'none' }], timing));
    }
    // The details' parts that come or go with a step: those that came fade in where they are, those that went fade out as copies.
    for (const [selector, a] of was.faded)
      if (!faded.has(selector)) {
        const g = ghostOf(a.el, a.box, at);
        copies.push(g);
        animations.push(g.animate([{ opacity: 1 }, { opacity: 0 }], timing));
      }
    for (const [selector, b] of faded) if (!was.faded.has(selector)) animations.push(b.el.animate([{ opacity: 0 }, { opacity: 1 }], timing));
    const sink = at.bottom - rest(gone, before);
    for (const k of gone) {
      const { copy, box } = before.get(k)!;
      // A part that is itself a shared one (the half queue's cover) has travelled already.
      if (travelled.has(copy.dataset.role ?? '')) continue;
      for (const name of travelled) for (const n of copy.querySelectorAll<HTMLElement>(`[data-role="${name}"]`)) n.style.visibility = 'hidden';
      const ride = under(box, before);
      leaving(k, ride ? dy(ride) : sink);
    }
    return {
      animations,
      distance: Math.max(distance, 100),
      /** Done (either way): the animations and copies go, leaving the layout as it is. */
      finish() {
        for (const a of animations) a.cancel();
        for (const c of copies) c.remove();
        for (const el of shown) el.style.visibility = '';
      },
    };
  }

  let sinking = $state<HTMLElement>();
  /** A drag on a handle is scrubbing a step change, which it animates itself. */
  let scrubbing = false;
  /** A step change is still playing out; a new drag waits for it rather than starting it over. */
  let busy = false;
  const still = () => matchMedia('(prefers-reduced-motion: reduce)').matches;
  $effect.pre(() => {
    void sheet.step;
    const root = untrack(() => section);
    if (scrubbing || !root || !views.phone || untrack(() => sheet.progress) !== 1 || still()) return;
    const before = seen(root, true);
    void tick().then(() => {
      if (sheet.progress !== 1) return;
      const change = stepChange(root, before, CURVE);
      busy = true;
      void Promise.allSettled(change.animations.map((a) => a.finished)).then(() => {
        change.finish();
        busy = false;
      });
    });
  });

  /**
   * Phones: a drag on one of the sheet's handles. Once it moves, the step
   * change it heads for (up towards the queue, back towards now playing)
   * follows the finger, then finishes or springs back on letting go, as
   * `released` decides. A tap calls `onTap`; a drag that would close the
   * sheet is the sheet's own (`closing`, else on letting go).
   */
  function stepDrag(e: PointerEvent, onTap: () => void, closing?: () => void) {
    if (e.button !== 0 || busy) return;
    if (!views.phone || !section || still()) return closing ? closing() : sheet.handle(e, onTap);
    const root = section;
    const y0 = e.clientY;
    const from = sheet.step;
    let way: 'up' | 'back' | null = null;
    let change: ReturnType<typeof stepChange> | undefined;
    let scrub: Scrub | undefined;
    const set = (step: SheetStep) => {
      scrubbing = true;
      sheet.step = step;
      flushSync();
      scrubbing = false;
    };
    const move = (ev: PointerEvent) => {
      const dy = ev.clientY - y0;
      if (!way) {
        if (Math.abs(dy) < SLOP) return;
        way = dy < 0 ? 'up' : 'back';
        const to = stepTo(from, way);
        if (to === 'closed' && closing) {
          stop();
          return closing();
        }
        if (!to || to === 'closed') return;
        const before = seen(root, true);
        set(to);
        change = stepChange(root, before, 'linear');
        scrub = new Scrub(change.animations);
      }
      if (change) scrub?.seek((way === 'up' ? -dy : dy) / change.distance);
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
    };
    const end = (ev: PointerEvent) => {
      stop();
      const what = ev.type === 'pointercancel' ? null : released(ev.clientY - y0);
      if (!scrub || !change) {
        if (what === 'tap') onTap();
        else if (what === 'up') sheet.up();
        else if (what === 'back') sheet.back();
        return;
      }
      const goes = what === way;
      const c = change;
      busy = true;
      // On from where the finger left it, either way.
      scrub.release(goes, () => {
        c.finish();
        if (!goes) set(from);
        busy = false;
      });
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }

  sheet.measure = measure;
  // Writes styles only; reads no state it writes.
  $effect(() => apply(sheet.progress, sheet.dragging));

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && sheet.progress === 1) sheet.collapse();
  }

  /**
   * Phones: a drag on now playing closes it (down, the sheet's drag) or
   * brings up the queue (up, following the finger like the handle); the seek
   * bar keeps its own, and the artwork unless its drag is remapped away from this.
   */
  function dragNow(e: PointerEvent) {
    if (!views.phone || sheet.step !== 'now') return;
    const own = mapped(settings.current.gestures, 'artwork')['drag-vertical'] !== 'close';
    if ((e.target as Element).closest(own ? '.art, [role="slider"], .more' : '[role="slider"], .more')) return;
    stepDrag(e, () => {}, () => sheet.drag(e, 'sheet', undefined, true));
  }

  /** The keyboard's press on a handle; pointers go through `sheet.handle`. */
  const keyed = (e: MouseEvent, then: () => void) => e.detail === 0 && then();
  const album = $derived(engine.current?.track.album ?? '');
  let coverWidth = $state(0);
</script>

<svelte:window onkeydown={onKey} />

<!--
  Morphs out of the player bar (or floating player): the background grows from
  its box and the artwork, title, transport and seek bar move from their places
  in it, all by `progress`, so dragging scrubs the morph. Built from the same
  widgets layouts use.
-->
<section
  bind:this={section}
  class="sheet"
  data-step={sheet.step}
  class:open={sheet.open}
  class:dragging={sheet.dragging}
  aria-label="Now playing"
  aria-hidden={!sheet.open}
  inert={!sheet.open}
>
  <div class="backdrop" bind:this={backdrop}><div class="tint" bind:this={tint}></div></div>
  <div class="ghosts" bind:this={ghosts} inert aria-hidden="true"></div>
  <!-- Copies of the parts a step change drops, sinking behind the rest. -->
  <div class="sinking" bind:this={sinking} inert aria-hidden="true"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header onpointerdown={(e) => sheet.drag(e, 'sheet')}>
    <span class="handle"></span>
    <button
      class="icon-button lyrics-toggle"
      class:active={sheet.lyrics}
      aria-label="Lyrics"
      aria-pressed={sheet.lyrics}
      title={sheet.lyrics ? 'Show artwork' : 'Show lyrics'}
      onclick={() => sheet.toggleLyrics()}
    >
      <Icon name="lyrics" size={18} />
    </button>
    <button class="icon-button" aria-label="Collapse" title="Collapse (Esc)" onclick={() => sheet.collapse()}>
      <Icon name="chevron-down" size={20} />
    </button>
  </header>

  <!-- Phones, full queue and lyrics: the playing track in a strip whose drag or tap steps back. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="mini" onpointerdown={(e) => !(e.target as Element).closest('.transport') && stepDrag(e, () => sheet.back())}>
    <div class="mini-row">
      <Artwork track={engine.current?.track} size={48} />
      <span class="mini-names">
        <b>{engine.current ? trackTitle(engine.current.track) : 'Nothing playing'}</b>
        <span>{engine.current?.track.artist ?? ''}</span>
      </span>
      <Transport compact size={22} />
    </div>
    <button class="step" aria-label={sheet.lyrics ? 'Back to now playing' : 'Show less of the queue'} onclick={(e) => keyed(e, () => sheet.back())}></button>
  </div>

  <div class="body" bind:this={body}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="now" onpointerdown={dragNow}>
      <!-- Phones, half queue: the cover, full width, behind the title. -->
      {#if views.phone && sheet.step === 'half'}
        <div class="cover-bg" aria-hidden="true" bind:clientWidth={coverWidth}>
          {#if coverWidth}<Artwork track={engine.current?.track} size={coverWidth} />{/if}
          <span class="shade"></span>
        </div>
      {/if}
      <div class="art" class:lyrics={sheet.lyrics}>
        {#if sheet.lyrics}<Lyrics />{/if}
        <!-- Kept laid out, unseen, while the lyrics show: drawn afresh (rebuilt, or from display: none),
             the carousel's cards fade in (its @starting-style), which blinked the cover on closing them. -->
        <div class="keep" hidden={sheet.lyrics}><ArtworkWidget expands={false} playing /></div>
      </div>
      <div class="details">
        <p class="album-title" use:marquee={sheet.progress === 1 ? (engine.current?.track.id ?? null) : null}><span class="marquee">{album}</span></p>
        <TrackInfo style="hero" playing>
          {#snippet aside()}
            <button class="lyrics-button" aria-label="Lyrics" aria-pressed={sheet.lyrics} title="Show lyrics" onclick={() => sheet.toggleLyrics()}>
              <Icon name="lyrics" size={22} />
            </button>
          {/snippet}
        </TrackInfo>
        <Controls />
        <!-- Phones: the handle at the bottom; a tap or drag up shows the queue. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="more" onpointerdown={(e) => stepDrag(e, () => sheet.up())}>
          <button class="step" aria-label="Show the queue" onclick={(e) => keyed(e, () => sheet.up())}></button>
        </div>
      </div>
    </div>
    <div class="queue-slot">
      <button
        class="step"
        aria-label="Show all of the queue"
        onpointerdown={(e) => stepDrag(e, () => sheet.up())}
        onclick={(e) => keyed(e, () => sheet.up())}
      ></button>
      <Queue collapsible={!views.phone} nowPlaying={views.phone} />
    </div>
  </div>
  <Scrollbar target={body} />
</section>

<style>
  .sheet {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    /* Hidden once the collapse has finished; the fade is the reduced-motion one. */
    visibility: hidden;
    transition:
      opacity 150ms linear,
      visibility 0s 260ms;
  }

  .sheet.open {
    visibility: visible;
    transition: opacity 150ms linear;
  }

  .sheet.dragging {
    transition: none;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    overflow: hidden;
    background: var(--bg);
    transform-origin: 0 0;
    will-change: transform;
  }

  /* The bar's colour, fading into the sheet's as it grows. */
  .tint {
    position: absolute;
    inset: 0;
    background: var(--surface);
    opacity: 0;
  }

  /* Copies of the bar's parts, on top while they crossfade. */
  .ghosts {
    position: absolute;
    inset: 0;
    z-index: 1;
    pointer-events: none;
  }

  .sinking {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
  }

  /* Let the artwork travel to the bar instead of being clipped by its box. */
  .art :global(.artwork) {
    overflow: visible;
    background: none;
  }

  header {
    position: relative;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    height: 40px;
    padding: 0 var(--space-2);
    flex: none;
    touch-action: none;
    cursor: grab;
  }

  /* Phone parts. */
  .mini,
  .album-title,
  .lyrics-button,
  .more,
  .queue-slot > .step {
    display: none;
  }

  .handle {
    position: absolute;
    left: 50%;
    top: 8px;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--border);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* Artwork beside the details; together they take whatever the queue leaves. */
  .now {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(300px, 460px) minmax(0, 0.4fr);
    align-items: center;
    gap: var(--space-6);
    padding: 0 var(--space-6) var(--space-6);
  }

  .art {
    height: 100%;
    min-height: 0;
  }

  /* Lyrics are as large as the space allows, not square like the artwork. */
  @media (min-width: 701px) {
    .now:has(.art.lyrics) {
      grid-template-columns: minmax(0, 1.5fr) minmax(300px, 460px) minmax(0, 0.2fr);
    }
  }

  .art.lyrics {
    align-self: stretch;
  }

  /* Lays the artwork out as if it were the art's own child. */
  .keep {
    display: contents;
  }

  .art.lyrics {
    position: relative;
  }

  .keep[hidden] {
    display: block;
    position: absolute;
    inset: 0;
    visibility: hidden;
    pointer-events: none;
  }

  .art.lyrics :global(.lyrics) {
    background: none;
  }

  .lyrics-toggle.active {
    color: var(--accent);
  }

  .details {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  /* The queue sits underneath: a header when folded, ~40% of the height when open. */
  .queue-slot {
    flex: none;
    border-top: 1px solid var(--divider);
  }

  .queue-slot:has(:global(.queue.open)) {
    flex: 0 1 40%;
    min-height: 160px;
  }

  /* Phones: cover the whole screen; artwork, details and queue stack and scroll. */
  @media (max-width: 700px) {
    .sheet {
      position: fixed;
      z-index: 45;
      padding-top: env(safe-area-inset-top);
      padding-bottom: env(safe-area-inset-bottom);
    }

    /* Doesn't scroll: the queue and lyrics scroll inside it. */
    .body {
      position: relative;
      overflow: hidden;
    }

    /* Fills the screen, so the handle sits at the bottom; long titles shrink the cover instead of pushing it off. */
    .now {
      flex: 1 1 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
      align-items: stretch;
      gap: var(--space-4);
      padding: 0 var(--space-4) var(--space-2);
    }

    /* The screen's width (the carousel keeps a small margin), plus room for its dots below. */
    .art {
      flex: 0 1 min(calc(100vw + 28px), 66vh);
      min-height: 40vw;
      height: auto;
      margin-inline: calc(-1 * var(--space-4));
    }

    .details {
      flex: 1 0 auto;
    }

    /* The handles step the sheet; Android's back and swipes close it. */
    header {
      display: none;
    }

    .now {
      padding-top: var(--space-2);
    }

    [data-step='now'] .now {
      touch-action: none;
    }

    .step {
      display: block;
      width: 100%;
      height: 28px;
      padding: 0;
      border: 0;
      background: none;
      cursor: grab;
      touch-action: none;
    }

    .step::before {
      content: '';
      display: block;
      width: 44px;
      height: 5px;
      margin: 0 auto;
      border-radius: 3px;
      background: var(--border);
    }

    /* One line each, so the controls never move between tracks; what doesn't fit scrolls (`marquee.ts`). */
    .album-title {
      display: flex;
      height: calc(1.3em + 0.4em);
      margin: 0;
      font-size: 1.5em;
      font-weight: 700;
      line-height: 1.3;
      overflow: hidden;
      white-space: nowrap;
      /* Room in the clipping box for ascenders and descenders, taken back outside it. */
      padding-block: 0.2em;
      margin-block: -0.2em;
    }

    /* Centred while it fits; auto margins drop to 0 when it overflows, so it starts at the left. */
    .album-title .marquee {
      flex: none;
      margin-inline: auto;
    }

    /* Clipping one-line boxes (`marquee.ts`), with room for ascenders and descenders (a 'g') taken back outside them. */
    .details :global(.hero :is(h1, .hero-artist)) {
      overflow: hidden;
      white-space: nowrap;
      padding-block: 0.2em;
    }

    .details :global(.hero h1) {
      margin-block: -0.2em;
    }

    .details :global(.hero .hero-artist) {
      margin-block: calc(var(--space-1) - 0.2em) -0.2em;
    }

    .details :global(.marquee) {
      display: inline-block;
    }

    .details :global(.hero-album) {
      min-height: 1.6em;
    }

    /* The controls sit low, just above the handle; the space goes between them and the names. */
    .details > :global(.controls) {
      margin-top: auto;
    }

    .lyrics-button {
      display: grid;
      place-items: center;
      flex: none;
      width: 46px;
      height: 46px;
      padding: 0;
      border: 0;
      border-radius: var(--radius-lg, 12px);
      background: var(--text);
      color: var(--bg);
      cursor: pointer;
    }

    /* The album's name sits above the title; under it, the format badge alone. */
    .details :global(.album-name) {
      display: none;
    }

    .details :global(.hero-album) {
      justify-content: center;
    }

    .details :global(.hero .format) {
      padding: 2px var(--space-3);
      border: 0;
      border-radius: 999px;
      background: var(--text);
      color: var(--bg);
      text-transform: uppercase;
    }

    .details :global(.hero .rates) {
      display: inline;
    }

    .more {
      display: block;
      padding: var(--space-3) 0;
      touch-action: none;
    }

    .queue-slot {
      display: none;
      border-top: 0;
    }

    /* Half and full: the queue fills what is left and scrolls on its own; the rest stays put. */

    [data-step='half'] .now {
      flex: none;
    }

    .queue-slot:has(:global(.queue.open)) {
      flex: 1 1 0;
      min-height: 0;
      flex-direction: column;
    }

    .queue-slot :global(.queue) {
      flex: 1;
      min-height: 0;
    }

    /* Half: the cover fills the top behind the title, the queue below the controls. */
    [data-step='half'] .now {
      position: relative;
      padding-top: calc(min(100vw, 62vh) - 7em);
    }

    /* Kept laid out (not display: none) so the carousel needn't rebuild on the way back. */
    [data-step='half'] .art {
      position: absolute;
      inset: 0 0 auto;
      height: min(100vw, 62vh);
      visibility: hidden;
    }

    /* Square and full width, its bottom where the 62vh band ends: a taller cover runs off the top. */
    .cover-bg {
      position: absolute;
      left: 0;
      right: 0;
      top: calc(min(100vw, 62vh) - 100vw);
      z-index: -1;
    }

    .cover-bg :global(.art) {
      border-radius: 0;
    }

    .cover-bg .shade {
      position: absolute;
      inset: 0;
      background: linear-gradient(transparent 45%, var(--bg));
    }

    [data-step='half'] :is(.album-title, .more),
    [data-step='half'] .details :global(.hero-album) {
      display: none;
    }

    [data-step='half'] .queue-slot,
    [data-step='full'] .queue-slot {
      display: flex;
    }

    [data-step='half'] .queue-slot > .step {
      display: block;
    }

    /* Full queue and lyrics: the strip on top, the rest below it. */
    :is([data-step='full'], [data-step='lyrics']) .mini {
      display: block;
      flex: none;
      padding-top: var(--space-3);
      background: var(--surface);
      touch-action: none;
    }

    .mini-row {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: 0 var(--space-3) var(--space-2) var(--space-4);
    }

    .mini-names {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
    }

    .mini-names > * {
      overflow-wrap: anywhere;
    }

    .mini-names span {
      color: var(--text-muted);
    }

    [data-step='full'] .now {
      display: none;
    }

    [data-step='lyrics'] .now {
      flex: 1;
      min-height: 0;
      padding: 0;
    }

    [data-step='lyrics'] .details {
      display: none;
    }

    /* A flex box, as WebKit leaves a percentage height unresolved in a flex item. */
    [data-step='lyrics'] .art.lyrics {
      flex: 1 1 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
    }

    [data-step='lyrics'] .art.lyrics :global(.lyrics) {
      flex: 1;
      min-height: 0;
      height: auto;
    }
  }
</style>
