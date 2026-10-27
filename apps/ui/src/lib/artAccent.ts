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

/** Lightness (0 to 1) of a `#rgb` or `#rrggbb` colour. */
function lightnessOf(color: string): number {
  let h = color.slice(1);
  if (h.length === 3) h = [...h].map((c) => c + c).join('');
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
  return rgbToHsl(r, g, b)[2];
}

const BUCKETS = 12;

/**
 * The cover's main colour and its mean brightness. The hue is the heaviest
 * of twelve hue buckets (weighted to the colourful pixels), averaged within
 * that bucket only: averaging across hues turns blue and yellow into green.
 * `hsl` is null for a greyscale or empty image.
 */
export function dominant(pixels: ArrayLike<number>): { hsl: [number, number, number] | null; brightness: number } {
  const sums = Array.from({ length: BUCKETS }, () => [0, 0, 0, 0]);
  let luma = 0, n = 0;
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
    if (pixels[i + 3] < 128) continue;
    luma += (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
    n++;
    const sat = Math.max(r, g, b) - Math.min(r, g, b);
    if (sat < 24) continue;
    const w = sat * sat;
    const bucket = sums[Math.floor(((rgbToHsl(r, g, b)[0] + 360 / BUCKETS / 2) % 360) / (360 / BUCKETS))];
    bucket[0] += r * w; bucket[1] += g * w; bucket[2] += b * w; bucket[3] += w;
  }
  const brightness = n ? luma / n : 0.5;
  const [r, g, b, w] = sums.reduce((best, s) => (s[3] > best[3] ? s : best));
  return { hsl: w ? rgbToHsl(r / w, g / w, b / w) : null, brightness };
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
 * 3:1) against `bg`. Null for a greyscale or empty image: the theme's own
 * accent is better than a grey one.
 */
export function accentFromPixels(pixels: ArrayLike<number>, bg: string): string | null {
  const { hsl } = dominant(pixels);
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
 * The interface colours for a cover. `tint` keeps the theme's light or dark
 * and shifts its backgrounds toward the cover's hue; `full` takes the
 * backgrounds and text from the cover, light or dark by its brightness.
 * Text stays readable (4.5:1, muted 3:1) on every background. Null when the
 * cover gives nothing to use: `tint` of a greyscale image changes nothing.
 */
export function artPalette(pixels: ArrayLike<number>, theme: Pick<Theme, 'scheme' | 'tokens'>, mode: 'tint' | 'full'): ArtPalette | null {
  const { hsl, brightness } = dominant(pixels);
  if (!hsl && mode === 'tint') return null;
  const h = hsl?.[0] ?? 0;
  // A full theme shows a bold cover's colour in the background itself.
  const sat = !hsl ? 0 : mode === 'full' ? Math.min(0.7, Math.max(0.15, hsl[1])) : Math.min(0.4, Math.max(0.12, hsl[1] * 0.5));
  const scheme = mode === 'tint' ? theme.scheme : brightness > 0.5 ? 'light' : 'dark';
  const lighter = scheme === 'dark';
  const tokens: Partial<Tokens> = {};
  for (const key of SURFACES) {
    tokens[key] = hslToHex(h, sat, mode === 'full' ? FULL_L[scheme][key] : lightnessOf(theme.tokens[key]));
  }
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
  return { scheme, tokens, accent: hsl && accentOn(hsl, tokens.bg!) };
}
