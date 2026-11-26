import { describe, expect, it } from 'vitest';
import { HOLD_MS, settle } from './seekHold';

describe('settle', () => {
  const sent = { ms: 60_000, at: 1_000 };

  it('holds the target while the engine still reports the old position', () => {
    expect(settle(sent, 10_000, 2_000)).toBe(sent);
  });

  it('lets go once the engine reports near the target', () => {
    expect(settle(sent, 60_200, 2_000)).toBeNull();
  });

  it('lets go after a while, if the seek never lands (it failed)', () => {
    expect(settle(sent, 10_000, 1_000 + HOLD_MS + 1)).toBeNull();
  });

  it('holds nothing when nothing was sent', () => {
    expect(settle(null, 10_000, 2_000)).toBeNull();
  });
});
