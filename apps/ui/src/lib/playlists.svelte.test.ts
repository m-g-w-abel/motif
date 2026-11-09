import { describe, expect, it } from 'vitest';
import { moveCommand } from './playlists.svelte';

describe('playlists', () => {
  const items = ['a', 'b', 'c'];
  it('names a drag by the entry it lands before, so a repeat lands it in the same place', () => {
    expect(moveCommand(items, 0, 2)).toEqual({ item: 'a', before: null });
    expect(moveCommand(items, 2, 0)).toEqual({ item: 'c', before: 'a' });
    expect(moveCommand(items, 0, 1)).toEqual({ item: 'a', before: 'c' });
  });
});
