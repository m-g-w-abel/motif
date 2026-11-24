import { describe, expect, it } from 'vitest';
import { dragScroll } from './scrollbarDrag';

describe('dragScroll', () => {
  // A 500px bar with a 100px thumb: 400px of travel over a 4000px range.
  it('moves the thumb 1:1 with the finger', () => {
    expect(dragScroll(0, 100, 1, 500, 100, 4000)).toBe(1000);
  });

  it('divides the root zoom out of the finger distance', () => {
    expect(dragScroll(0, 200, 2, 500, 100, 4000)).toBe(1000);
  });

  it('follows the range as it changes mid-drag', () => {
    expect(dragScroll(100, 100, 1, 500, 80, 8000)).toBe((200 / 420) * 8000);
  });

  it('stops at both ends', () => {
    expect(dragScroll(0, -50, 1, 500, 100, 4000)).toBe(0);
    expect(dragScroll(0, 900, 1, 500, 100, 4000)).toBe(4000);
  });
});
