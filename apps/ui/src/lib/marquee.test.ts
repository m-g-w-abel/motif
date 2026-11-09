import { describe, expect, it } from 'vitest';
import { MIN_MS, SPEED, scrollPlan } from './marquee';

describe('scrollPlan', () => {
  it('leaves text that fits (or is a pixel over, from rounding) alone', () => {
    expect(scrollPlan(0)).toBeNull();
    expect(scrollPlan(-30)).toBeNull();
    expect(scrollPlan(1)).toBeNull();
  });

  it('scrolls by the overflow at a steady speed', () => {
    expect(scrollPlan(SPEED * 3)).toEqual({ distance: SPEED * 3, ms: 3000 });
  });

  it('takes a minimum time for a short overflow', () => {
    expect(scrollPlan(5)).toEqual({ distance: 5, ms: MIN_MS });
  });
});
