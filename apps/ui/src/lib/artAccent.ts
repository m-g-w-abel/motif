import { contrast } from './themeFile';
import type { Theme } from './themes';

/** How much of the cover's colour the interface takes. */
export type ArtColours = 'off' | 'accent' | 'tint' | 'full';

const hex = (n: number) => Math.round(n).toString(16).padStart(2, '0');

function hslToHex(h: number, s: number, l: number): string {
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return 255 * (l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
  };
  return `#${hex(f(0))}${hex(f(8))}${hex(f(4))}`;
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  [r, g, b] = [r / 255, g / 255, b / 255];
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  const l = (max + min) / 2;
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  const h = (d === 0 ? 0 : max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4) * 60;
  return [h, s, l];
}

type Rgb = [number, number, number];

function rgbOf(color: string): Rgb {
  let h = color.slice(1);
  if (h.length === 3) h = [...h].map((c) => c + c).join('');
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as Rgb;
}

/** Lightness (0 to 1) of a `#rgb` or `#rrggbb` colour. */
const lightnessOf = (color: string) => rgbToHsl(...rgbOf(color))[2];

const chromaOf = ([r, g, b]: Rgb) => Math.max(r, g, b) - Math.min(r, g, b);

/** OKLab of an sRGB colour, for grouping colours as people see them. */
function oklab(r: number, g: number, b: number): Rgb {
  const lin = (c: number) => ((c /= 255) <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  [r, g, b] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787995 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}

const BUCKETS = 12;
// Below this spread between channels a pixel counts as grey.
const GREY = 24;

/**
 * The cover's colours. `area` is the mean of the coarse OKLab bucket covering
 * most of the image, greys and whites included: the background. `hues` are the
 * colourful pixels in twelve hue buckets (weighted by saturation²), heaviest
 * first, each averaged within its bucket only: averaging across hues turns blue
 * and yellow into green. `area` is null and `hues` empty for an empty image.
 */
export function dominant(pixels: ArrayLike<number>): { area: Rgb | null; hues: Rgb[] } {
  const sums = Array.from({ length: BUCKETS }, () => [0, 0, 0, 0]);
  const areas = new Map<number, number[]>();
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
    if (pixels[i + 3] < 128) continue;
    const [L, A, B] = oklab(r, g, b);
    const key = Math.min(5, Math.floor(L * 6)) * 400 + Math.round(A / 0.08 + 10) * 20 + Math.round(B / 0.08 + 10);
    const area = areas.get(key) ?? areas.set(key, [0, 0, 0, 0]).get(key)!;
    area[0] += r; area[1] += g; area[2] += b; area[3]++;
    const sat = chromaOf([r, g, b]);
    if (sat < GREY) continue;
    const w = sat * sat;
    const bucket = sums[Math.floor(((rgbToHsl(r, g, b)[0] + 360 / BUCKETS / 2) % 360) / (360 / BUCKETS))];
    bucket[0] += r * w; bucket[1] += g * w; bucket[2] += b * w; bucket[3] += w;
  }
  const mean = ([r, g, b, w]: number[]): Rgb => [r / w, g / w, b / w];
  const top = [...areas.values()].reduce<number[] | null>((best, a) => (!best || a[3] > best[3] ? a : best), null);
  return { area: top && mean(top), hues: sums.filter((s) => s[3]).sort((a, b) => b[3] - a[3]).map(mean) };
}

/**
 * The accent's colour: the heaviest hue not too close to the background's
 * (a cover of one hue keeps it; `accentOn` then moves its lightness). Null
 * for a greyscale cover: the theme's own accent is better than a grey one.
 */
function accentHsl(hues: Rgb[], bg: Rgb): [number, number, number] | null {
  const bgHue = rgbToHsl(...bg)[0];
  // Only a clearly colourful background rules out its hue: orange still reads on dull brown.
  const far = (c: Rgb) => chromaOf(bg) < 2 * GREY || 180 - Math.abs(Math.abs(rgbToHsl(...c)[0] - bgHue) - 180) > 40;
  const pick = hues.find(far) ?? hues[0];
  return pick ? rgbToHsl(...pick) : null;
}

/** Moves `l` toward lighter or darker until the colour reaches `min` contrast on every one of `bgs`. */
function readable(h: number, s: number, l: number, bgs: string[], min: number, lighter: boolean): string {
  let out = hslToHex(h, s, l);
  while (bgs.some((bg) => contrast(out, bg) < min) && l > 0 && l < 1) {
    l = Math.min(1, Math.max(0, l + (lighter ? 0.02 : -0.02)));
    out = hslToHex(h, s, l);
  }
  return out;
}

function accentOn([h, s, l]: [number, number, number], bg: string): string {
  // Lighter on dark backgrounds, darker on light ones, until it reads.
  return readable(h, Math.min(1, Math.max(0.45, s)), Math.min(0.9, Math.max(0.1, l)), [bg], 3, lightnessOf(bg) < 0.5);
}

/**
 * An accent colour for RGBA `pixels` (a small thumbnail), readable (at least
 * 3:1) against `bg`. Null for a greyscale or empty image.
 */
export function accentFromPixels(pixels: ArrayLike<number>, bg: string): string | null {
  const hsl = accentHsl(dominant(pixels).hues, rgbOf(bg));
  return hsl && accentOn(hsl, bg);
}

type Tokens = Theme['tokens'];

const SURFACES = ['bg', 'surface', 'surface-2', 'border', 'divider'] as const;
// Lightness of each surface token in a full theme, dark and light.
const FULL_L = {
  dark: { bg: 0.2, surface: 0.24, 'surface-2': 0.29, border: 0.33, divider: 0.45 },
  light: { bg: 0.86, surface: 0.91, 'surface-2': 0.81, border: 0.75, divider: 0.62 },
};

/** What the interface takes from a cover: tokens to lay over the theme's, and an accent (null: keep the theme's). */
export interface ArtPalette {
  scheme: Theme['scheme'];
  tokens: Partial<Tokens>;
  accent: string | null;
}

/**
 * The interface colours for a cover. The backgrounds come from the colour
 * covering most of the cover, the accent from its strongest other hue. `tint`
 * keeps the theme's light or dark and shifts its backgrounds toward that
 * colour; `full` takes the backgrounds and text from it, light or dark by its
 * brightness. Text stays readable (4.5:1, muted 3:1) on every background.
 * Null when the cover gives nothing to use: `tint` of a greyscale image
 * changes nothing.
 */
export function artPalette(pixels: ArrayLike<number>, theme: Pick<Theme, 'scheme' | 'tokens'>, mode: 'tint' | 'full'): ArtPalette | null {
  const { area, hues } = dominant(pixels);
  if (!area || (!hues.length && mode === 'tint')) return null;
  const [h] = rgbToHsl(...area);
  // The background's colourfulness, kept as its lightness changes: a near-white cover stays near neutral.
  const chroma = chromaOf(area) / 255;
  const satAt = (l: number) => {
    const s = chroma / Math.max(0.01, 1 - Math.abs(2 * l - 1));
    return mode === 'full' ? Math.min(0.7, s) : Math.min(0.4, s * 0.5);
  };
  const scheme = mode === 'tint' ? theme.scheme : (0.2126 * area[0] + 0.7152 * area[1] + 0.0722 * area[2]) / 255 > 0.5 ? 'light' : 'dark';
  const lighter = scheme === 'dark';
  const tokens: Partial<Tokens> = {};
  for (const key of SURFACES) {
    const l = mode === 'full' ? FULL_L[scheme][key] : lightnessOf(theme.tokens[key]);
    tokens[key] = hslToHex(h, satAt(l), l);
  }
  const sat = satAt(lightnessOf(tokens.bg!));
  const bgs = [tokens.bg!, tokens.surface!, tokens['surface-2']!];
  if (mode === 'full') {
    tokens.hover = lighter ? 'rgb(255 255 255 / 0.05)' : 'rgb(0 0 0 / 0.06)';
    tokens.shadow = lighter ? '0 8px 32px rgb(0 0 0 / 0.5)' : '0 6px 24px rgb(0 0 0 / 0.18)';
    tokens.text = readable(h, Math.min(sat, 0.3), lighter ? 0.94 : 0.1, bgs, 4.5, lighter);
    tokens['text-muted'] = readable(h, Math.min(sat, 0.3), lighter ? 0.75 : 0.28, bgs, 3, lighter);
  } else {
    // A new hue shifts luminance a little, so re-check the theme's own text.
    for (const [key, min] of [['text', 4.5], ['text-muted', 3]] as const) {
      const c = theme.tokens[key];
      if (bgs.some((bg) => contrast(c, bg) < min)) tokens[key] = readable(h, 0, lightnessOf(c), bgs, min, lighter);
    }
  }
  const accent = accentHsl(hues, area);
  return { scheme, tokens, accent: accent && accentOn(accent, tokens.bg!) };
}
