import { describe, expect, it } from 'vitest';
import { navTarget, scrollInto, vimTarget, type NavKey } from './keynav';

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

describe('vimTarget', () => {
  const v = (k: string, pending = '', index = 5, count = 20, cols = 1, mods: Partial<NavKey> = {}) =>
    vimTarget(key(k, mods), pending, index, count, cols, 10);
  it('moves with j/k, and h/l only in a grid', () => {
    expect(v('j').to).toBe(6);
    expect(v('k').to).toBe(4);
    expect(v('k', '', 0).to).toBe(0);
    expect(v('l').handled).toBe(false);
    expect(v('j', '', 5, 20, 4).to).toBe(9);
    expect(v('l', '', 5, 20, 4).to).toBe(6);
    expect(v('h', '', 5, 20, 4).to).toBe(4);
  });
  it('takes counts', () => {
    const a = v('1');
    expect(a).toEqual({ pending: '1', handled: true, to: null });
    const b = v('2', a.pending);
    expect(b.pending).toBe('12');
    expect(v('j', '3').to).toBe(8);
    expect(v('j', '30').to).toBe(19);
    expect(v('0').handled).toBe(false);
    expect(v('G', '3').to).toBe(2);
  });
  it('goes to the ends with gg and G', () => {
    const g = v('g');
    expect(g).toEqual({ pending: 'g', handled: true, to: null });
    expect(v('g', g.pending).to).toBe(0);
    expect(v('g', '4g').to).toBe(3);
    expect(v('G').to).toBe(19);
  });
  it('pages by half with Ctrl-d/u', () => {
    expect(v('d', '', 5, 20, 1, { ctrlKey: true }).to).toBe(10);
    expect(v('u', '', 5, 20, 1, { ctrlKey: true }).to).toBe(0);
    expect(v('x', '', 5, 20, 1, { ctrlKey: true }).handled).toBe(false);
  });
  it('drops pending input on other keys and ignores empty lists', () => {
    expect(v('x', '3')).toEqual({ pending: '', handled: false, to: null });
    expect(vimTarget(key('j'), '', 0, 0, 1, 10).handled).toBe(false);
  });
});
