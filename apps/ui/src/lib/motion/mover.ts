import { boxFlight, css, fitted, textFlight, type Box, type Fit } from './fit';
import type { Track } from './transition';

/**
 * The shared-element mover (docs/ANIMATION.md): carries an element seen
 * before a change to its counterpart after it, as copies in a layer above
 * everything, so neither side's scrolling or clipping cuts it off. Text
 * travels as the new drawing in a clip box that goes from the old box to
 * the new, with the old drawing fading over it where the two look different;
 * anything else as a copy of whichever end's image is sharper.
 */

/** What a copy keeps of its original's look, which came partly from rules on its ancestors. */
const LOOK = [
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'font-stretch',
  'font-variant',
  'line-height',
  'letter-spacing',
  'text-transform',
  'text-align',
  'color',
  'white-space',
  'overflow',
  'text-overflow',
  'border-radius',
  'background-color',
  'box-sizing',
  'padding-top',
  'padding-right',
  'padding-bottom',
  'padding-left',
  'flex-direction',
  'flex-wrap',
  'gap',
  'justify-content',
  'align-items',
];
/** Differences that show between two drawings of the same words. */
const STYLE = ['color', 'font-weight', 'font-style', 'font-family'];
/** What must match where a copy lands. */
const LANDING = [...STYLE, 'font-size'];
/** Keyframes for text: the clip's stretch and the words' counter-stretch don't interpolate linearly. */
const STEPS = 8;

/** An element as it was seen: where, what it looks like, how sharp its image is. */
export type Seen = {
  el: HTMLElement;
  box: DOMRect;
  /** What shows of it: its box cut by the boxes that clip it (a marquee's line), where it rests. */
  view: DOMRect;
  glyph: DOMRect;
  ink: number;
  text: boolean;
  look: Record<string, string>;
  cut: boolean;
  res: number;
  /** A copy made while it was drawn (`cloneOf`), for when a change takes it away before it flies. */
  copy?: HTMLElement;
  /** It lay behind its stage's content (`behind`), seen while it was drawn. */
  behind?: boolean;
};

/** Whether `el` lies behind the content of the stage `root`: it or a box it is in has a negative z-index there (the half queue's cover). */
export function behind(el: HTMLElement, root: HTMLElement) {
  for (let e: HTMLElement | null = el; e && e !== root; e = e.parentElement) {
    const s = getComputedStyle(e);
    if (s.position !== 'static' && parseInt(s.zIndex) < 0) return true;
  }
  return false;
}

/** Whether all of `el`'s words sit on one line. */
function oneLine(el: Element) {
  const range = document.createRange();
  range.selectNodeContents(el);
  const lines = new Set([...range.getClientRects()].map((q) => Math.round(q.top)));
  return lines.size === 1;
}

/** Copies of images in the motion layer: they travel under words. */
const pictures = new WeakSet<Element>();

/** The box of the first letter or digit, or of the element without one. */
export function glyph(el: HTMLElement): DOMRect {
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

/** The loaded image's width inside `el` (0: none yet). */
const resolution = (el: HTMLElement) => {
  const img = el.querySelector('img');
  return img?.complete ? img.naturalWidth : 0;
};

export function see(el: HTMLElement, box = el.getBoundingClientRect()): Seen {
  const text = !el.firstElementChild && !!el.textContent?.trim();
  const style = getComputedStyle(el);
  const look: Record<string, string> = {};
  for (const p of LOOK) look[p] = style.getPropertyValue(p);
  // A copy is a box of the element's size. An inline element's box is its font's own height, while its line
  // height may be taller: as a block, the words would sit centred in that taller line, lower than the original's.
  if (style.display === 'inline') {
    look.display = 'block';
    look['line-height'] = 'inline';
  } else look.display = style.display;
  const g = text ? glyph(el) : box;
  // Measured where it will rest: shift the glyph with the box when the box was corrected for a moving screen.
  const raw = el.getBoundingClientRect();
  const glyphAt = new DOMRect(g.x + box.x - raw.x, g.y + box.y - raw.y, g.width, g.height);
  const range = document.createRange();
  range.selectNodeContents(el);
  // Cut by every box that clips it, shifted like the box when that was corrected for a moving screen.
  let [l, t, r, b] = [box.left, box.top, box.right, box.bottom];
  for (let up = el.parentElement; up && up !== document.body; up = up.parentElement) {
    const s = getComputedStyle(up);
    if (s.overflowX === 'visible' && s.overflowY === 'visible') continue;
    const c = up.getBoundingClientRect();
    if (s.overflowX !== 'visible') [l, r] = [Math.max(l, c.left + box.x - raw.x), Math.min(r, c.right + box.x - raw.x)];
    if (s.overflowY !== 'visible') [t, b] = [Math.max(t, c.top + box.y - raw.y), Math.min(b, c.bottom + box.y - raw.y)];
  }
  const view = r > l && b > t ? new DOMRect(l, t, r - l, b - t) : box;
  // Words on one line stay on one line in a copy, whose width is a fraction of a pixel off the original's.
  if (oneLine(el) && look['white-space'] === 'normal') look['white-space'] = 'nowrap';
  return { el, box, view, glyph: glyphAt, ink: range.getBoundingClientRect().width, text, look, cut: el.scrollWidth > el.clientWidth + 1, res: resolution(el) };
}

let layer: HTMLElement | undefined;
/** Where copies travel: fixed, above everything, never in the way of a press. */
export function motionLayer() {
  if (!layer?.isConnected) {
    layer = document.createElement('div');
    layer.className = 'motion-layer';
    layer.ariaHidden = 'true';
    Object.assign(layer.style, { position: 'fixed', left: '0', top: '0', width: '0', height: '0', overflow: 'visible', zIndex: '46', pointerEvents: 'none' });
    document.body.append(layer);
  }
  return layer;
}

/** The root's zoom (the Scale setting, and Android's font size): measured once per value. */
let zoom = { key: '', value: 1 };
export function rootZoom() {
  const key = document.documentElement.style.zoom;
  if (key !== zoom.key) {
    const probe = document.createElement('div');
    probe.style.width = '100px';
    motionLayer().append(probe);
    zoom = { key, value: probe.getBoundingClientRect().width / 100 || 1 };
    probe.remove();
  }
  return zoom.value;
}

/** Copies whose insides keep their look (`cloneOf`), so a part inside one can be copied from it. */
const looked = new WeakSet<Element>();
export const full = (copy: Element) => looked.has(copy);

/** What a copy's insides keep of how they look (the copy's own box takes all of `LOOK`). */
const INNER = [
  'display',
  'visibility',
  'opacity',
  'font-family',
  'font-size',
  'font-weight',
  'font-style',
  'line-height',
  'letter-spacing',
  'text-transform',
  'text-align',
  'color',
  'white-space',
  'background-color',
  'background-image',
  'border-radius',
  // As drawn now: a marquee mid-scroll is where its transition has got to, not where it is going.
  'transform',
];
/** Its box, so it is laid out as it was (a row a rule on the sheet sizes): for what isn't inline. */
const BOX = ['box-sizing', 'width', 'height', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left'];
/** How a flex or grid box lays out its insides (a row centred by a rule on the sheet), and how its items take part. */
const FLEX = ['gap', 'flex-direction', 'flex-wrap', 'justify-content', 'align-items', 'grid-template-columns', 'grid-template-rows'];
const ITEM = ['flex-grow', 'flex-shrink', 'flex-basis'];
const laysOut = (display: string) => /flex|grid/.test(display);
/** Past this many elements inside, a copy keeps only its own rules (a long list: its rows' rules are their own). */
const MOST_INSIDE = 300;

/**
 * A deep copy of `el` whose insides look as they do there: rules that reach
 * them through the original's ancestors (a phone rule hiding the album's
 * name, a font set on the sheet) don't reach a copy elsewhere.
 */
export function cloneOf(el: HTMLElement): HTMLElement {
  const c = el.cloneNode(true) as HTMLElement;
  // A copy only moves as its transition draws it: no CSS transitions or `@starting-style` fades of its own.
  c.style.setProperty('transition', 'none');
  const inside = el.querySelectorAll<HTMLElement>('*');
  const copies = c.querySelectorAll<HTMLElement>('*');
  // A copy starts at the top of what scrolls: where each part of it was scrolled, put back once it is in the page (`rescroll`).
  [el, ...inside].forEach((o, i) => {
    if (o.scrollTop || o.scrollLeft) (i ? copies[i - 1] : c).dataset.scroll = `${o.scrollLeft} ${o.scrollTop}`;
  });
  if (inside.length > MOST_INSIDE) return c;
  looked.add(c);
  inside.forEach((o, i) => {
    const n = copies[i];
    n.style.setProperty('transition', 'none');
    const style = getComputedStyle(o);
    // Undrawn insides stay undrawn (a box of `display: contents` has no boxes of its own, but its insides draw).
    if (style.display !== 'contents' && !o.getClientRects().length) return void n.style.setProperty('display', 'none');
    const display = style.display;
    const props = [...INNER, ...(display.startsWith('inline') && !laysOut(display) ? [] : BOX), ...(laysOut(display) ? FLEX : []), ...(o.parentElement && laysOut(getComputedStyle(o.parentElement).display) ? ITEM : [])];
    // Visible is inherited, not set: a part hidden in the copy (one that flies) hides what it holds too.
    for (const p of props) if (p !== 'visibility' || style.visibility !== 'visible') n.style.setProperty(p, style.getPropertyValue(p));
    // Words of its own on one line stay on one line.
    if (style.whiteSpace === 'normal' && [...o.childNodes].some((c) => c.nodeType === Node.TEXT_NODE && c.textContent!.trim()) && oneLine(o)) n.style.setProperty('white-space', 'nowrap');
  });
  return c;
}

/** Scrolls a copy that is now in the page as its original was scrolled (`cloneOf`). */
export function rescroll(copy: HTMLElement) {
  for (const n of [copy, ...copy.querySelectorAll<HTMLElement>('[data-scroll]')]) {
    if (!n.dataset.scroll) continue;
    const [left, top] = n.dataset.scroll.split(' ').map(Number);
    Object.assign(n, { scrollLeft: left, scrollTop: top });
  }
}

/** A copy of `s.el` (or `clone`, made of it earlier), looking as it does there, at `at` (absolute in its parent, or fixed on screen). */
export function copyOf(s: Seen, at: Box, fixed: boolean, clone = (s.copy?.cloneNode(true) as HTMLElement | undefined) ?? cloneOf(s.el)) {
  const c = clone;
  for (const n of [c, ...c.querySelectorAll('[data-move], [data-enter]')]) n.removeAttribute('data-move'), n.removeAttribute('data-enter');
  // The original's lazy, async image would decode afresh and draw nothing for a frame.
  for (const img of c.querySelectorAll('img')) Object.assign(img, { loading: 'eager', decoding: 'sync' });
  for (const [p, v] of Object.entries(s.look)) c.style.setProperty(p, v);
  // Its line exactly as tall as the inline box (see `see`), so the words sit where they did.
  if (s.look['line-height'] === 'inline') c.style.lineHeight = `${at.height}px`;
  Object.assign(c.style, {
    position: fixed ? 'fixed' : 'absolute',
    left: `${at.left}px`,
    top: `${at.top}px`,
    width: `${at.width}px`,
    height: `${at.height}px`,
    margin: '0',
    transformOrigin: '0 0',
    visibility: 'visible',
    transform: 'none',
    opacity: '1',
  });
  return c;
}

/** What an element is made of (its elements and their classes), to tell two drawings of different shapes apart. */
const shape = (el: Element): string => `${el.tagName}.${[...el.classList].sort().join('.')}(${[...el.children].map(shape).join()})`;

/** Whether `el` is drawn as it will stay: its artwork (if any) loaded, or settled on its placeholder. */
function drawn(el: HTMLElement) {
  const img = el.querySelector('img');
  return !img || (img.complete && img.naturalWidth > 0);
}

/** A shared element's journey: its tracks, then where it ends. */
export interface Flight {
  tracks: Track[];
  /** The transition ended: at the new place (`landed`), or back at the old. */
  done(landed: boolean): void;
}

/** Dev builds: each landing's error, for the landing check (`scripts/landing.mjs`). */
export const landings: { name: string; box: number; look: string[] }[] | null = import.meta.env?.DEV ? [] : null;

/**
 * Carries `from` to `to`, which sits at `toBox` once its screen is at rest.
 * The copies show (and the originals hide) once their images have decoded,
 * so nothing ever draws empty; at the end the new element shows once its own
 * image has decoded (a second at most), then the copies go.
 */
/**
 * Where a change's copies travel, besides the motion layer above everything:
 * `back`, a layer behind the stage's content (above its backdrop), for an
 * end that lies behind it (`behind`).
 */
export interface Layers {
  root: HTMLElement;
  back?: HTMLElement;
}

export function fly(from: Seen, to: HTMLElement, toBox: DOMRect, layers?: Layers): Flight {
  const dest = see(to, toBox);
  // Copies are placed in CSS pixels, which the root's zoom scales; boxes are measured on screen.
  const z = rootZoom();
  const px = (b: DOMRect) => new DOMRect(b.x / z, b.y / z, b.width / z, b.height / z);
  const [fb, fg, tb, tg] = [px(from.box), px(from.glyph), px(toBox), px(dest.glyph)];
  const root = motionLayer();
  const tracks: Track[] = [];
  const copies: HTMLElement[] = [];
  /** Where each copy travels, when not in the motion layer. */
  const layerOf = new Map<HTMLElement, HTMLElement>();
  /** The copies that start where `from` is and end where `to` is. */
  let starts!: HTMLElement;
  let lands!: HTMLElement;
  if (from.text && dest.text) {
    // Whole words at both ends (one line, not cut off): their widths, so another weight doesn't ghost.
    const whole = (x: Seen) => !x.cut && x.box.height < 2 * x.glyph.height;
    const ink = whole(from) && whole(dest) ? { from: from.ink / z, to: dest.ink / z } : undefined;
    // In the same font, the sizes' ratio is exact; glyph boxes are whole pixels (a 5% hop on small text).
    const sizes = [from, dest].map((x) => parseFloat(x.look['font-size']));
    const scale = from.look['font-family'] === dest.look['font-family'] && sizes[0] && sizes[1] ? sizes[0] / sizes[1] : undefined;
    // The clip runs between what shows of each end (a marquee's line cuts a long title), the words where they are drawn.
    const [fv, tv] = [px(from.view), px(dest.view)];
    const f = textFlight({ from: fb, fromGlyph: fg, to: tb, toGlyph: tg, fromView: fv, toView: tv, steps: STEPS, ink, scale });
    const clip = document.createElement('div');
    Object.assign(clip.style, {
      position: 'fixed',
      left: `${tv.left}px`,
      top: `${tv.top}px`,
      width: `${tv.width}px`,
      height: `${tv.height}px`,
      overflow: 'hidden',
      transformOrigin: '0 0',
    });
    const words = copyOf(dest, { left: tb.left - tv.left, top: tb.top - tv.top, width: tb.width, height: tb.height }, false);
    clip.append(words);
    // Another colour or weight, or cut off or wrapped differently: the old drawing travels locked onto
    // the words and they crossfade all the way, so each end is exactly one drawing and the look changes gradually.
    const differ = from.cut !== dest.cut || STYLE.some((p) => from.look[p] !== dest.look[p]) || Math.abs(fb.height / fg.height - tb.height / tg.height) > 0.5;
    tracks.push({ el: clip, frames: f.clip.map((m) => ({ transform: css(m) })) }, { el: words, frames: f.dest.map((m, k) => ({ transform: css(m), ...(differ && { opacity: k / STEPS }) })) });
    if (differ) {
      const old = copyOf(from, { left: fb.left - tv.left, top: fb.top - tv.top, width: fb.width, height: fb.height }, false);
      clip.append(old);
      tracks.push({ el: old, frames: f.source.map((m, k) => ({ transform: css(m), opacity: 1 - k / STEPS })) });
    }
    copies.push(clip);
    lands = words;
    starts = clip.lastElementChild as HTMLElement;
  } else {
    // The sharper drawing already loaded travels (the grid's or the page's).
    const own = from.res > 0 && from.res >= dest.res ? 'from' : 'to';
    const s = own === 'from' ? from : dest;
    const fit = (to.dataset.fit ?? from.el.dataset.fit ?? 'box') as Fit;
    // Shapes other than a box: the drawing starts on (or ends on) the part of the old box its shape fits.
    const [a, b] = own === 'from' ? [fb, fitted(tb, fb, fit)] : [fitted(fb, tb, fit), tb];
    const c = copyOf(s, own === 'from' ? a : b, true);
    // Two drawings of other shapes and no image (the bar's thin seek bar and the sheet's waveform): the old
    // travels too, stretched the same way, and they crossfade all the way, so neither look shows at the wrong end.
    // Each end at its place in the stage's stack: one behind the content travels behind it (so titles and
    // shades stay over a cover); ends at different places travel as two drawings, one at each, crossfading.
    const [fromBack, toBack] = layers?.back ? [!!from.behind, behind(to, layers.root)] : [false, false];
    const differ = fromBack !== toBack || (!from.res && !dest.res && !from.el.querySelector('img') && shape(from.el) !== shape(to));
    if (toBack) layerOf.set(c, layers!.back!);
    tracks.push({ el: c, frames: boxFlight(a, b, own).frames.map((m, k) => ({ transform: css(m), ...(differ && { opacity: k }) })) });
    copies.push(c);
    lands = starts = c;
    if (differ) {
      const old = copyOf(from, fb, true);
      if (fromBack) layerOf.set(old, layers!.back!);
      tracks.push({ el: old, frames: boxFlight(fb, tb, 'from').frames.map((m, k) => ({ transform: css(m), opacity: 1 - k })) });
      copies.unshift(old);
      starts = old;
    }
  }
  for (const c of copies) {
    c.style.visibility = 'hidden';
    const layer = layerOf.get(c);
    if (layer) layer.append(c);
    else if (c.querySelector('img') || c.tagName === 'IMG') {
      // Images under words: before the first copy that isn't one.
      pictures.add(c);
      root.insertBefore(c, [...root.children].find((k) => !pictures.has(k)) ?? null);
    } else root.append(c);
    rescroll(c);
  }
  // Shown with the originals hidden in one go, once decoded. Loaded already (the copies decode as they
  // paint): at once, so the real element is never drawn a frame where it isn't yet (a row moving up).
  const images = copies.flatMap((c) => [...c.querySelectorAll('img')]);
  let over = false;
  const swap = () => {
    if (over) return;
    for (const c of copies) c.style.visibility = 'visible';
    from.el.style.visibility = to.style.visibility = 'hidden';
  };
  if (images.every((i) => i.complete && i.naturalWidth)) swap();
  else void Promise.race([Promise.allSettled(images.map((i) => i.decode())), new Promise((r) => setTimeout(r, 100))]).then(swap);
  const name = to.dataset.move ?? '';
  return {
    tracks,
    done(landed) {
      over = true;
      from.el.style.visibility = '';
      // Held where the transition left them while the new image decodes (its animations are gone).
      for (const t of tracks) {
        const { transform, opacity } = (landed ? t.frames.at(-1) : t.frames[0]) as { transform?: string; opacity?: number };
        if (transform) t.el.style.transform = transform;
        if (opacity !== undefined) t.el.style.opacity = String(opacity);
      }
      // Back where it started (played backwards): the copy that shows first lies on `from`, as it does at the end.
      const [shows, real] = landed ? [lands, to] : [starts, from.el];
      if (landings && real.isConnected) {
        // Text: where its words are drawn, not its element's box (the two can sit differently inside their boxes).
        const drawnAt = (el: HTMLElement) => {
          if (!dest.text) return el.getBoundingClientRect();
          const range = document.createRange();
          range.selectNodeContents(el);
          return range.getBoundingClientRect();
        };
        const boxOf = (d: DOMRect): Box => ({ left: d.x, top: d.y, width: d.width, height: d.height });
        const a = boxOf(drawnAt(shows));
        // A drawing of another shape lies on the part of the box its shape fits (`fitted`).
        const fit = (to.dataset.fit ?? from.el.dataset.fit ?? 'box') as Fit;
        const b = dest.text ? boxOf(drawnAt(real)) : fitted(boxOf(drawnAt(real)), a, fit);
        const [la, lb] = [getComputedStyle(shows), getComputedStyle(real)];
        // The copy is drawn at its own size and scaled; compare what shows.
        const scale = shows.getBoundingClientRect().height / ((shows.offsetHeight || 1) * rootZoom());
        landings.push({
          name: landed ? name : `${name} (back)`,
          // Words by their left, width and centre line: the height of a run of text is its font's, rounded to whole pixels at each size.
          box: dest.text
            ? Math.max(Math.abs(a.left - b.left), Math.abs(a.width - b.width), Math.abs(a.top + a.height / 2 - b.top - b.height / 2))
            : Math.max(Math.abs(a.left - b.left), Math.abs(a.top - b.top), Math.abs(a.width - b.width), Math.abs(a.height - b.height)),
          look: (from.text ? LANDING : []).filter((p) => (p === 'font-size' ? Math.abs(parseFloat(la.fontSize) * scale - parseFloat(lb.fontSize)) > 0.5 : la.getPropertyValue(p) !== lb.getPropertyValue(p))),
        });
      }
      const since = performance.now();
      const handOver = () => {
        if (landed && to.isConnected && !drawn(to) && performance.now() - since < 1000) return void requestAnimationFrame(handOver);
        to.style.visibility = '';
        for (const c of copies) c.remove();
      };
      handOver();
    },
  };
}
