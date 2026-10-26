import { describe, expect, it } from 'vitest';
import { navTarget, scrollInto, type NavKey } from './keynav';

const key = (k: string, mods: Partial<NavKey> = {}): NavKey => ({ key: k, altKey: false, metaKey: false, ctrlKey: false, ...mods });

describe('navTarget in a list', () => {
  it('moves by one, a page, and to the ends, clamped', () => {
    expect(navTarget(key('ArrowDown'), 3, 10, 1, 4)).toBe(4);
    expect(navTarget(key('ArrowDown'), 9, 10, 1, 4)).toBe(9);
    expect(navTarget(key('ArrowUp'), 0, 10, 1, 4)).toBe(0);
    expect(navTarget(key('PageDown'), 3, 10, 1, 4)).toBe(7);
    expect(navTarget(key('PageDown'), 8, 10, 1, 4)).toBe(9);
    expect(navTarget(key('PageUp'), 3, 10, 1, 4)).toBe(0);
    expect(navTarget(key('Home'), 5, 10, 1, 4)).toBe(0);
    expect(navTarget(key('End'), 5, 10, 1, 4)).toBe(9);
  });
  it('ignores sideways keys, other keys, modified keys and empty lists', () => {
    expect(navTarget(key('ArrowLeft'), 5, 10, 1, 4)).toBeNull();
    expect(navTarget(key('a'), 5, 10, 1, 4)).toBeNull();
    expect(navTarget(key('ArrowDown', { altKey: true }), 5, 10, 1, 4)).toBeNull();
    expect(navTarget(key('ArrowDown'), 0, 0, 1, 4)).toBeNull();
  });
});

describe('navTarget in a grid of 4 columns, 10 items', () => {
  it('moves in two dimensions', () => {
    expect(navTarget(key('ArrowRight'), 1, 10, 4, 2)).toBe(2);
    expect(navTarget(key('ArrowLeft'), 0, 10, 4, 2)).toBe(0);
    expect(navTarget(key('ArrowDown'), 1, 10, 4, 2)).toBe(5);
    expect(navTarget(key('ArrowDown'), 7, 10, 4, 2)).toBe(9);
    expect(navTarget(key('ArrowUp'), 5, 10, 4, 2)).toBe(1);
    expect(navTarget(key('PageDown'), 1, 10, 4, 2)).toBe(9);
    expect(navTarget(key('PageUp'), 9, 10, 4, 1)).toBe(5);
  });
  it('Home/End go to the row ends, with Ctrl to the list ends', () => {
    expect(navTarget(key('Home'), 6, 10, 4, 2)).toBe(4);
    expect(navTarget(key('End'), 5, 10, 4, 2)).toBe(7);
    expect(navTarget(key('End'), 9, 10, 4, 2)).toBe(9);
    expect(navTarget(key('Home', { ctrlKey: true }), 6, 10, 4, 2)).toBe(0);
    expect(navTarget(key('End', { ctrlKey: true }), 1, 10, 4, 2)).toBe(9);
  });
});

describe('scrollInto', () => {
  it('scrolls only as far as needed', () => {
    expect(scrollInto(100, 200, 150, 180)).toBe(100);
    expect(scrollInto(100, 200, 50, 80)).toBe(50);
    expect(scrollInto(100, 200, 290, 320)).toBe(120);
  });
});
