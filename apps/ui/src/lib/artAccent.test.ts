import { describe, expect, it } from 'vitest';
import { accentFromPixels, artPalette } from './artAccent';
import { THEMES } from './themes';
import { contrast } from './themeFile';

const fill = (rgb: number[], n = 16) => Uint8ClampedArray.from(Array.from({ length: n }, () => [...rgb, 255]).flat());

describe('accentFromPixels', () => {
  it('is null for greyscale and transparent images', () => {
    expect(accentFromPixels(fill([120, 120, 120]), '#121214')).toBeNull();
    expect(accentFromPixels(Uint8ClampedArray.from([200, 30, 30, 0]), '#121214')).toBeNull();
    expect(accentFromPixels([], '#121214')).toBeNull();
  });

  it('keeps the hue and reads on dark and light backgrounds', () => {
    for (const bg of ['#121214', '#ffffff', '#000000', '#f4f1ea']) {
      for (const rgb of [[20, 20, 90], [250, 240, 60], [10, 60, 10]]) {
        const c = accentFromPixels(fill(rgb), bg)!;
        expect(contrast(c, bg)).toBeGreaterThanOrEqual(3);
      }
    }
    const red = accentFromPixels(fill([200, 30, 30]), '#121214')!;
    expect(parseInt(red.slice(1, 3), 16)).toBeGreaterThan(parseInt(red.slice(3, 5), 16));
  });

  it('prefers the colourful pixels over a grey majority', () => {
    const px = Uint8ClampedArray.from([...fill([90, 90, 90], 30), ...fill([30, 90, 220], 2)]);
    const c = accentFromPixels(px, '#121214')!;
    expect(parseInt(c.slice(5, 7), 16)).toBeGreaterThan(parseInt(c.slice(1, 3), 16));
  });
});

describe('artPalette', () => {
  const covers: [string, number[]][] = [['red', [200, 30, 30]], ['navy', [10, 15, 60]], ['cream', [250, 240, 200]], ['grey dark', [30, 30, 30]], ['grey light', [225, 225, 225]]];

  it('keeps text readable on every background, for every theme and cover', () => {
    for (const theme of THEMES) {
      for (const mode of ['tint', 'full'] as const) {
        for (const [name, rgb] of covers) {
          const p = artPalette(fill(rgb), theme, mode);
          if (!p) continue;
          const t = { ...theme.tokens, ...p.tokens };
          for (const bg of [t.bg, t.surface, t['surface-2']]) {
            expect(contrast(t.text, bg), `${theme.id} ${mode} ${name} text`).toBeGreaterThanOrEqual(4.5);
            expect(contrast(t['text-muted'], bg), `${theme.id} ${mode} ${name} muted`).toBeGreaterThanOrEqual(3);
          }
          if (p.accent) expect(contrast(p.accent, t.bg)).toBeGreaterThanOrEqual(3);
        }
      }
    }
  });

  it('tint keeps the theme light or dark; full follows the cover', () => {
    const graphite = THEMES.find((t) => t.id === 'graphite')!;
    expect(artPalette(fill([250, 240, 200]), graphite, 'tint')!.scheme).toBe('dark');
    expect(artPalette(fill([250, 240, 200]), graphite, 'full')!.scheme).toBe('light');
    expect(artPalette(fill([10, 15, 60]), THEMES.find((t) => t.scheme === 'light')!, 'full')!.scheme).toBe('dark');
  });

  it('tint of a greyscale cover changes nothing; full still follows its brightness', () => {
    const graphite = THEMES[0];
    expect(artPalette(fill([120, 120, 120]), graphite, 'tint')).toBeNull();
    const p = artPalette(fill([225, 225, 225]), graphite, 'full')!;
    expect(p.scheme).toBe('light');
    expect(p.accent).toBeNull();
  });
});

describe('hue choice', () => {
  const mix = (parts: [number[], number][]) => Uint8ClampedArray.from(parts.flatMap(([rgb, n]) => Array.from({ length: n }, () => [...rgb, 255]).flat()));
  const hue = (c: string) => {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
    const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    return (max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4) * 60;
  };

  it('a blue, grey and black cover (Contemporary Movement) is blue, not green', () => {
    const px = mix([[[40, 80, 170], 70], [[110, 112, 118], 80], [[10, 10, 12], 100], [[60, 90, 120], 6]]);
    expect(hue(accentFromPixels(px, '#121214')!)).toBeGreaterThan(200);
    expect(hue(accentFromPixels(px, '#121214')!)).toBeLessThan(240);
  });

  it('a yellow and black cover (Little Dark Age) is yellow, not green', () => {
    const px = mix([[[240, 205, 30], 60], [[120, 100, 15], 20], [[170, 130, 20], 14], [[8, 8, 8], 150]]);
    for (const bg of ['#121214', '#f5f1e8']) {
      expect(hue(accentFromPixels(px, bg)!)).toBeGreaterThan(35);
      expect(hue(accentFromPixels(px, bg)!)).toBeLessThan(65);
    }
  });

  it('a full theme from a bold cover is clearly coloured, not near black or white', () => {
    const p = artPalette(mix([[[200, 20, 30], 200]]), THEMES[0], 'full')!;
    const bg = p.tokens.bg!;
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(bg.slice(i, i + 2), 16));
    expect(r - Math.max(g, b)).toBeGreaterThan(40);
  });
});

describe('background and accent picked separately', () => {
  const mix = (parts: [number[], number][]) => Uint8ClampedArray.from(parts.flatMap(([rgb, n]) => Array.from({ length: n }, () => [...rgb, 255]).flat()));
  const rgb = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
  const chroma = (c: string) => Math.max(...rgb(c)) - Math.min(...rgb(c));
  const hue = (c: string) => {
    const [r, g, b] = rgb(c);
    const max = Math.max(r, g, b), d = max - Math.min(r, g, b);
    return (max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4) * 60;
  };

  it('a white cover with a blue streak gets a light neutral background and a blue accent', () => {
    const p = artPalette(mix([[[248, 248, 246], 200], [[30, 70, 210], 56]]), THEMES[0], 'full')!;
    expect(p.scheme).toBe('light');
    expect(chroma(p.tokens.bg!)).toBeLessThan(12);
    expect(hue(p.accent!)).toBeGreaterThan(200);
    expect(hue(p.accent!)).toBeLessThan(250);
    expect(contrast(p.accent!, p.tokens.bg!)).toBeGreaterThanOrEqual(3);
  });

  it('an all-blue cover gets a blue background and a readable blue accent', () => {
    const p = artPalette(mix([[[30, 70, 210], 256]]), THEMES[0], 'full')!;
    for (const c of [p.tokens.bg!, p.accent!]) {
      expect(chroma(c)).toBeGreaterThan(30);
      expect(hue(c)).toBeGreaterThan(200);
      expect(hue(c)).toBeLessThan(250);
    }
    expect(contrast(p.accent!, p.tokens.bg!)).toBeGreaterThanOrEqual(3);
  });

  it('the accent skips a hue too close to the background', () => {
    const p = artPalette(mix([[[200, 25, 30], 220], [[240, 210, 40], 36]]), THEMES[0], 'full')!;
    expect(hue(p.tokens.bg!) > 330 || hue(p.tokens.bg!) < 15).toBe(true);
    expect(hue(p.accent!)).toBeGreaterThan(35);
    expect(hue(p.accent!)).toBeLessThan(65);
  });

  it('a greyscale cover keeps the theme accent', () => {
    for (const mode of ['tint', 'full'] as const) {
      expect(artPalette(mix([[[250, 250, 250], 200], [[20, 20, 20], 56]]), THEMES[0], mode)?.accent ?? null).toBeNull();
    }
  });
});
