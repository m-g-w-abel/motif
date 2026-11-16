// The playing cover's sampled pixels (16×16 RGBA), kept so a restart starts
// in the album's colours instead of the theme's until the engine answers.
const KEY = 'art-pixels';
const LENGTH = 16 * 16 * 4;

/** The pixels saved last, or null if none (or broken, or no storage). */
export function lastArtPixels(): Uint8ClampedArray | null {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(KEY) ?? 'null');
    if (!Array.isArray(saved) || saved.length !== LENGTH || !saved.every((n) => typeof n === 'number')) return null;
    return Uint8ClampedArray.from(saved);
  } catch {
    return null;
  }
}

export function rememberArtPixels(pixels: Uint8ClampedArray | null) {
  try {
    if (pixels) localStorage.setItem(KEY, JSON.stringify(Array.from(pixels)));
    else localStorage.removeItem(KEY);
  } catch {
    // No storage (private window): the next start shows the theme's colours first.
  }
}
