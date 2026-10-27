import { describe, expect, it } from 'vitest';
import { accentFromPixels } from './artAccent';
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
