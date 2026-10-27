import { contrast } from './themeFile';

const hex = (n: number) => Math.round(n).toString(16).padStart(2, '0');

function hslToHex(h: number, s: number, l: number): string {
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    return 255 * (l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1)));
  };
  return `#${hex(f(0))}${hex(f(8))}${hex(f(4))}`;
}

/**
 * An accent colour for RGBA `pixels` (a small thumbnail), readable (at least
 * 3:1) against `bg`. Colourful pixels count for more, so a mostly dark cover
 * with a red logo gives red. Null for a greyscale or empty image: the theme's
 * own accent is better than a grey one.
 */
export function accentFromPixels(pixels: ArrayLike<number>, bg: string): string | null {
  let r = 0, g = 0, b = 0, total = 0;
  for (let i = 0; i + 3 < pixels.length; i += 4) {
    const [pr, pg, pb] = [pixels[i], pixels[i + 1], pixels[i + 2]];
    const sat = Math.max(pr, pg, pb) - Math.min(pr, pg, pb);
    if (pixels[i + 3] < 128 || sat < 24) continue;
    const w = sat * sat;
    r += pr * w; g += pg * w; b += pb * w; total += w;
  }
  if (!total) return null;
  [r, g, b] = [r / total / 255, g / total / 255, b / total / 255];
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let l = (max + min) / 2;
  const s = Math.min(1, Math.max(0.45, d / (1 - Math.abs(2 * l - 1))));
  const h = (d === 0 ? 0 : max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4) * 60;
  // Lighter on dark themes, darker on light ones, until it reads.
  const step = contrast('#ffffff', bg) > contrast('#000000', bg) ? 0.03 : -0.03;
  l = Math.min(0.9, Math.max(0.1, l));
  let out = hslToHex(h, s, l);
  while (contrast(out, bg) < 3 && l > 0.05 && l < 0.95) {
    l += step;
    out = hslToHex(h, s, l);
  }
  return out;
}
