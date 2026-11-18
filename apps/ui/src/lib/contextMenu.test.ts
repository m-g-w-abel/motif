import { describe, expect, it } from 'vitest';
import { menuOpens } from './contextMenu.svelte';

describe('menuOpens', () => {
  it('opens on a right-click anywhere, but never from a long press on a phone', () => {
    expect(menuOpens('mouse', false)).toBe(true);
    expect(menuOpens('mouse', true)).toBe(true);
    expect(menuOpens('touch', false)).toBe(true);
    expect(menuOpens('touch', true)).toBe(false);
    expect(menuOpens('pen', true)).toBe(false);
  });
});
