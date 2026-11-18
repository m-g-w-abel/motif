/**
 * Fades the root colour tokens to new values (theme switch, light/dark, cover
 * colours) in OKLab, so the midpoints look halfway (#201). A JS tween rather
 * than CSS transitions: the tokens are hex and `rgb()`, which CSS would mix in sRGB.
 */

import { CURVE, DURATION, still } from './motion/transition';

type Rgba = [number, number, number, number];

const COLOUR = /#[\da-f]{3,8}\b|rgba?\([^)]*\)/gi;

/** sRGB 0–255 and alpha 0–1 of a hex or `rgb()` colour. */
function parse(css: string): Rgba | null {
  if (css[0] === '#') {
    let h = css.slice(1);
    if (h.length <= 4) h = [...h].map((c) => c + c).join('');
    if (h.length !== 6 && h.length !== 8) return null;
    const n = [0, 2, 4, 6].map((i) => parseInt(h.slice(i, i + 2) || 'ff', 16));
    return [n[0], n[1], n[2], n[3] / 255];
  }
  const n = css.match(/[\d.]+%?/g)?.map((v) => (v.endsWith('%') ? parseFloat(v) / 100 : Number(v)));
  return n && n.length >= 3 ? [n[0], n[1], n[2], n[3] ?? 1] : null;
}

const toLinear = (c: number) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const fromLinear = (c: number) => 255 * (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

// Björn Ottosson's OKLab matrices.
function toOklab([r, g, b]: Rgba): [number, number, number] {
  [r, g, b] = [toLinear(r), toLinear(g), toLinear(b)];
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function fromOklab([L, a, b]: [number, number, number]): [number, number, number] {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ].map((c) => Math.min(255, Math.max(0, fromLinear(c)))) as [number, number, number];
}

const round = (n: number, places: number) => Math.round(n * 10 ** places) / 10 ** places;

function mixColour(from: Rgba, to: Rgba, t: number): string {
  const [a, b] = [toOklab(from), toOklab(to)];
  const [r, g, bl] = fromOklab([0, 1, 2].map((i) => a[i] + (b[i] - a[i]) * t) as [number, number, number]);
  return `rgb(${round(r, 1)} ${round(g, 1)} ${round(bl, 1)} / ${round(from[3] + (to[3] - from[3]) * t, 4)})`;
}

/**
 * A token `t` of the way from one value to another: every colour in it mixed
 * in OKLab when the two have the same shape (a shadow keeps its offsets), the
 * new value straight away when they don't.
 */
export function mixTokens(from: string, to: string, t: number): string {
  if (t <= 0) return from;
  if (t >= 1 || from === to) return to;
  const [fromColours, toColours] = [from.match(COLOUR) ?? [], to.match(COLOUR) ?? []].map((cs) => cs.map(parse));
  if (!toColours.length || from.replace(COLOUR, '') !== to.replace(COLOUR, '') || fromColours.length !== toColours.length) return to;
  if ([...fromColours, ...toColours].some((c) => !c)) return to;
  let i = 0;
  return to.replace(COLOUR, () => mixColour(fromColours[i]!, toColours[i++]!, t));
}

/** Writes tokens through `write`, fading from what is shown; one frame loop per change, none once it lands. */
export class ColourFade {
  #shown = new Map<string, string>();
  #frame = 0;

  constructor(private write: (token: string, value: string) => void) {}

  to(targets: Record<string, string>) {
    cancelAnimationFrame(this.#frame);
    const entries = Object.entries(targets).filter(([k, v]) => this.#shown.get(k) !== v);
    if (!entries.length) return;
    // The first paint and reduced motion snap.
    const from = this.#shown.size && !still() ? new Map(this.#shown) : null;
    const start = performance.now();
    const step = (now: number) => {
      const t = from ? Math.min(1, (now - start) / DURATION) : 1;
      for (const [k, v] of entries) {
        const value = mixTokens(from?.get(k) ?? v, v, CURVE.at(t));
        if (value !== this.#shown.get(k)) this.write(k, value);
        this.#shown.set(k, value);
      }
      if (t < 1) this.#frame = requestAnimationFrame(step);
    };
    if (from) this.#frame = requestAnimationFrame(step);
    else step(start);
  }
}
