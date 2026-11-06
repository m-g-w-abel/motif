import { describe, expect, it } from 'vitest';
import { closeTop } from './back';

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
