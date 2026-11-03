import { describe, expect, it } from 'vitest';
import { lineAt, nextStart } from './lyrics.svelte';

const lines = [1000, 2500, 4000].map((time_ms, i) => ({ time_ms, text: `Line ${i}` }));

describe('lineAt', () => {
  it('picks the last line started', () => {
    expect(lineAt(lines, 0, 0)).toBe(-1);
    expect(lineAt(lines, 1000, 0)).toBe(0);
    expect(lineAt(lines, 2499, 0)).toBe(0);
    expect(lineAt(lines, 2500, 0)).toBe(1);
    expect(lineAt(lines, 99_000, 0)).toBe(2);
    expect(lineAt([], 5000, 0)).toBe(-1);
  });

  it('moves lines later with a positive offset and earlier with a negative one', () => {
    expect(lineAt(lines, 1200, 300)).toBe(-1);
    expect(lineAt(lines, 1300, 300)).toBe(0);
    expect(lineAt(lines, 2000, -500)).toBe(1);
    expect(nextStart(lines, 0, -500)).toBe(2000);
    expect(nextStart(lines, -1, 300)).toBe(1300);
    expect(nextStart(lines, 2, 0)).toBeNull();
  });
});
