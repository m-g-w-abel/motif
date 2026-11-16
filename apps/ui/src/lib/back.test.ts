import { describe, expect, it } from 'vitest';
import { backSwipe, closeTop } from './back';

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
  it('drives the top open layer by the system back swipe, and springs it back when cancelled', () => {
    const shown: (number | string)[] = [];
    const drive = { seek: (p: number) => shown.push(p), release: (commit: boolean) => shown.push(`release ${commit}`) };
    const below = { open: true, close() {}, follow: () => ({ seek: () => shown.push(-1), release() {} }) };
    const swipe = backSwipe(() => [{ open: false, close() {} }, { open: true, close() {}, follow: () => drive }, below]);
    swipe.start();
    swipe.progress(0.25);
    swipe.progress(0.5);
    swipe.cancel();
    expect(shown).toEqual([0.25, 0.5, 'release false']);
  });
});
