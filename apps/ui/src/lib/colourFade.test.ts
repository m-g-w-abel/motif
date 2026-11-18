import { describe, expect, it, vi } from 'vitest';
import { ColourFade, mixTokens } from './colourFade';
import { DURATION } from './motion/transition';

const channels = (css: string) => css.match(/[\d.]+/g)!.map(Number);

describe('mixing colour tokens', () => {
  it('gives exactly the old value at the start and the new at the end', () => {
    expect(mixTokens('#121214', '#f4f4f6', 0)).toBe('#121214');
    expect(mixTokens('#121214', '#f4f4f6', 1)).toBe('#f4f4f6');
  });

  it('passes through the OKLab midpoint, not the sRGB one', () => {
    // Black to white: OKLab L 0.5 is sRGB ~99, where an sRGB mix gives 128.
    const [r, g, b, a] = channels(mixTokens('#000000', '#ffffff', 0.5));
    for (const c of [r, g, b]) expect(c).toBeCloseTo(99.3, 0);
    expect(a).toBe(1);
    // Red to blue keeps its chroma through purple instead of a dull sRGB mix.
    const [r2, g2, b2] = channels(mixTokens('#ff0000', '#0000ff', 0.5));
    expect([r2, g2, b2].map(Math.round)).toEqual([140, 83, 162]);
  });

  it('mixes alpha and the colours inside a shadow of the same shape', () => {
    expect(channels(mixTokens('rgb(255 255 255 / 0.05)', '#ffffff0d', 0.5)).at(-1)).toBeCloseTo(0.0505, 3);
    const shadow = mixTokens('0 8px 32px rgb(0 0 0 / 0.5)', '0 8px 32px rgb(0 0 0 / 0.1)', 0.5);
    expect(shadow).toMatch(/^0 8px 32px rgb\(0 0 0 \/ 0\.3\)$/);
  });

  it('switches straight to values it cannot mix', () => {
    expect(mixTokens('0 8px 32px rgb(0 0 0 / 0.5)', '0 6px 24px rgb(0 0 0 / 0.18)', 0.5)).toBe('0 6px 24px rgb(0 0 0 / 0.18)');
  });
});

describe('fading the root tokens', () => {
  it('snaps the first paint, then fades a change over frames and stops once it lands', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (f: FrameRequestCallback) => frames.push(f));
    vi.stubGlobal('cancelAnimationFrame', () => {});
    vi.spyOn(performance, 'now').mockReturnValue(0);
    const writes: [string, string][] = [];
    const fade = new ColourFade((k, v) => writes.push([k, v]));

    fade.to({ bg: '#000000', text: '#ffffff' });
    expect(writes).toEqual([['bg', '#000000'], ['text', '#ffffff']]);
    expect(frames).toHaveLength(0);

    writes.length = 0;
    fade.to({ bg: '#ffffff', text: '#ffffff' });
    frames.shift()!(DURATION / 2);
    expect(writes).toHaveLength(1);
    expect(writes[0][1]).toMatch(/^rgb\(/);
    frames.shift()!(DURATION);
    expect(writes.at(-1)).toEqual(['bg', '#ffffff']);
    expect(frames).toHaveLength(0);
    vi.unstubAllGlobals();
  });
});
