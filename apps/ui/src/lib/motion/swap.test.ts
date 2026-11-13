import { describe, expect, it } from 'vitest';
import { swap } from './swap';

describe('swap', () => {
  it('animates only when its key changes: not on mount, not when the same key comes again (data arriving)', () => {
    let animated = 0;
    const animation = { pause() {}, play() {}, cancel() {}, currentTime: 0, playbackRate: 1, playState: 'paused', finished: new Promise(() => {}) };
    const el = { animate: () => (animated++, animation) } as unknown as HTMLElement;
    const action = swap(el, 'a|All');
    expect(animated).toBe(0);
    action.update('a|All');
    expect(animated).toBe(0);
    action.update('ab|All');
    expect(animated).toBe(1);
  });
});
