import { describe, expect, it } from 'vitest';
import { inSelection, ordered, span, toggle, type ItemAt } from './selection';

const items = ['a', 'b', 'c', 'd'];
const at: ItemAt<string> = (i) => (items[i] ? [items[i], items[i].toUpperCase()] : undefined);

describe('selection', () => {
  it('picks a range either way round', () => {
    expect([...span(at, 3, 1).keys()]).toEqual(['b', 'c', 'd']);
    expect([...span(at, 1, 3).keys()]).toEqual(['b', 'c', 'd']);
  });

  it('toggles items and keeps list order', () => {
    let picked = span(at, 2, 2);
    picked = toggle(picked, at, 0);
    expect(ordered(picked)).toEqual(['A', 'C']);
    picked = toggle(picked, at, 2);
    expect(ordered(picked)).toEqual(['A']);
  });

  it('applies actions to the selection only for a picked item of several', () => {
    const picked = span(at, 0, 1);
    expect(inSelection(picked, 'a')).toBe(true);
    expect(inSelection(picked, 'c')).toBe(false);
    expect(inSelection(span(at, 0, 0), 'a')).toBe(false);
  });
});
