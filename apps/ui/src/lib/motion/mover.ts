import { boxFlight, css, textFlight, type Box } from './fit';
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
];
/** Differences that show between two drawings of the same words. */
const STYLE = ['color', 'font-weight', 'font-style', 'font-family'];
/** What must match where a copy lands. */
const LANDING = [...STYLE, 'font-size'];
/** Keyframes for text: the clip's stretch and the words' counter-stretch don't interpolate linearly. */
const STEPS = 8;

/** An element as it was seen: where, what it looks like, how sharp its image is. */
export type Seen = { el: HTMLElement; box: DOMRect; glyph: DOMRect; ink: number; text: boolean; look: Record<string, string>; cut: boolean; res: number };

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
  look.display = style.display === 'inline' ? 'block' : style.display;
  const g = text ? glyph(el) : box;
  // Measured where it will rest: shift the glyph with the box when the box was corrected for a moving screen.
  const raw = el.getBoundingClientRect();
  const glyphAt = new DOMRect(g.x + box.x - raw.x, g.y + box.y - raw.y, g.width, g.height);
  const range = document.createRange();
  range.selectNodeContents(el);
  return { el, box, glyph: glyphAt, ink: range.getBoundingClientRect().width, text, look, cut: el.scrollWidth > el.clientWidth + 1, res: resolution(el) };
}

let layer: HTMLElement | undefined;
/** Where copies travel: fixed, above everything, never in the way of a press. */
function motionLayer() {
  if (!layer?.isConnected) {
    layer = document.createElement('div');
    layer.className = 'motion-layer';
    layer.ariaHidden = 'true';
    Object.assign(layer.style, { position: 'fixed', left: '0', top: '0', width: '0', height: '0', overflow: 'visible', zIndex: '40', pointerEvents: 'none' });
    document.body.append(layer);
  }
  return layer;
}

let zoom: number | undefined;
/**
 * The text zoom a phone's WebView applies to any font size set (Android's
 * font scale, e.g. 1.1): computed sizes already include it, so a copy given
 * them would be zoomed twice. Measured once.
 */
function textZoom() {
  if (zoom === undefined) {
    const probe = document.createElement('div');
    probe.style.fontSize = '100px';
    document.body.append(probe);
    zoom = parseFloat(getComputedStyle(probe).fontSize) / 100 || 1;
    probe.remove();
  }
  return zoom;
}
/** The value to set for `p` so it computes to `v` again (font size and line height are zoomed). */
const unzoomed = (p: string, v: string) => (p !== 'font-size' && p !== 'line-height') || !v.endsWith('px') ? v : `${parseFloat(v) / textZoom()}px`;

/** A copy of `s.el`, looking as it does there, at `at` (absolute in `parent`, or fixed on screen). */
function copyOf(s: Seen, at: Box, fixed: boolean) {
  const c = s.el.cloneNode(true) as HTMLElement;
  c.removeAttribute('data-move');
  // The original's lazy, async image would decode afresh and draw nothing for a frame.
  for (const img of c.querySelectorAll('img')) Object.assign(img, { loading: 'eager', decoding: 'sync' });
  for (const [p, v] of Object.entries(s.look)) c.style.setProperty(p, unzoomed(p, v));
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
export function fly(from: Seen, to: HTMLElement, toBox: DOMRect): Flight {
  const dest = see(to, toBox);
  const root = motionLayer();
  const tracks: Track[] = [];
  const copies: HTMLElement[] = [];
  /** The copy that ends where `to` is. */
  let lands!: HTMLElement;
  if (from.text && dest.text) {
    // Whole words at both ends (one line, not cut off): their widths, so another weight doesn't ghost.
    const whole = (x: Seen) => !x.cut && x.box.height < 2 * x.glyph.height;
    const ink = whole(from) && whole(dest) ? { from: from.ink, to: dest.ink } : undefined;
    const f = textFlight({ from: from.box, fromGlyph: from.glyph, to: toBox, toGlyph: dest.glyph, steps: STEPS, ink });
    const clip = document.createElement('div');
    Object.assign(clip.style, {
      position: 'fixed',
      left: `${toBox.left}px`,
      top: `${toBox.top}px`,
      width: `${toBox.width}px`,
      height: `${toBox.height}px`,
      overflow: 'hidden',
      transformOrigin: '0 0',
    });
    const words = copyOf(dest, { left: 0, top: 0, width: toBox.width, height: toBox.height }, false);
    clip.append(words);
    // Another colour or weight, or cut off or wrapped differently: the old drawing travels locked onto
    // the words and they crossfade all the way, so each end is exactly one drawing and the look changes gradually.
    const differ = from.cut !== dest.cut || STYLE.some((p) => from.look[p] !== dest.look[p]) || Math.abs(from.box.height / from.glyph.height - toBox.height / dest.glyph.height) > 0.5;
    tracks.push({ el: clip, frames: f.clip.map((m) => ({ transform: css(m) })) }, { el: words, frames: f.dest.map((m, k) => ({ transform: css(m), ...(differ && { opacity: k / STEPS }) })) });
    if (differ) {
      const old = copyOf(from, { left: from.box.left - toBox.left, top: from.box.top - toBox.top, width: from.box.width, height: from.box.height }, false);
      clip.append(old);
      tracks.push({ el: old, frames: f.source.map((m, k) => ({ transform: css(m), opacity: 1 - k / STEPS })) });
    }
    copies.push(clip);
    lands = words;
  } else {
    // The sharper drawing already loaded travels (the grid's or the page's).
    const own = from.res > 0 && from.res >= dest.res ? 'from' : 'to';
    const s = own === 'from' ? from : dest;
    const c = copyOf(s, s.box, true);
    tracks.push({ el: c, frames: boxFlight(from.box, toBox, own).frames.map((m) => ({ transform: css(m) })) });
    copies.push(c);
    lands = c;
  }
  for (const c of copies) {
    c.style.visibility = 'hidden';
    root.append(c);
  }
  // Shown with the originals hidden in one go, once decoded (a cached image: at once).
  const images = copies.flatMap((c) => [...c.querySelectorAll('img')]);
  const decoded = Promise.race([Promise.allSettled(images.map((i) => i.decode())), new Promise((r) => setTimeout(r, 100))]);
  let over = false;
  void decoded.then(() => {
    if (over) return;
    for (const c of copies) c.style.visibility = 'visible';
    from.el.style.visibility = to.style.visibility = 'hidden';
  });
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
      if (landed && landings && to.isConnected) {
        const a = lands.getBoundingClientRect();
        const b = to.getBoundingClientRect();
        const [la, lb] = [getComputedStyle(lands), getComputedStyle(to)];
        // The copy is drawn at its own size and scaled; compare what shows.
        const scale = a.height / (lands.offsetHeight || 1);
        landings.push({
          name,
          box: Math.max(Math.abs(a.x - b.x), Math.abs(a.y - b.y), Math.abs(a.width - b.width), Math.abs(a.height - b.height)),
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
