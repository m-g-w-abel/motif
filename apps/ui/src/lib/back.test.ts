import { describe, expect, it } from 'vitest';
import { backSwipe, closeTop } from './back';
import type { Gesture } from './gestures';

describe('closeTop', () => {
  it('closes only the first open layer', () => {
    const closed: string[] = [];
    const layer = (name: string, open: boolean) => ({ open, close: () => closed.push(name) });
    expect(closeTop([layer('menu', false), layer('dialog', true), layer('sheet', true)])).toBe(true);
    expect(closed).toEqual(['dialog']);
  });

  it('reports nothing open, so back can leave the app', () => {
    let closed = false;
    expect(closeTop([{ open: false, close: () => (closed = true) }])).toBe(false);
    expect(closed).toBe(false);
  });
});

describe('backSwipe', () => {
  it("moves the top open layer with the system back swipe, across the screen's width, and springs it back when cancelled", () => {
    const shown: (number | Gesture | null)[] = [];
    const slide = { move: (dx: number) => shown.push(dx), end: (swipe: Gesture | null) => shown.push(swipe) };
    const below = { open: true, close() {}, follow: () => ({ move: () => shown.push(-1), end() {} }) };
    const swipe = backSwipe(() => [{ open: false, close() {} }, { open: true, close() {}, follow: () => slide }, below], () => 400);
    swipe.start();
    swipe.progress(0.25);
    swipe.progress(0.5);
    swipe.cancel();
    expect(shown).toEqual([100, 200, null]);
  });
});
