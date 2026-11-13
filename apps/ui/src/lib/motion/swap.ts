import { Transition } from './transition';

/**
 * Svelte action: what `el` holds rises into place when `key` changes (a new
 * search or filter, the Songs/Albums switch). Only then: not on mount, and
 * not as data for the same key arrives in pieces.
 */
export function swap(el: HTMLElement, key: string) {
  let t: Transition | null = null;
  return {
    update(next: string) {
      if (next === key) return;
      key = next;
      t?.end();
      t = new Transition([{ el, frames: [{ opacity: 0.4, transform: 'translateY(12px)' }, { opacity: 1, transform: 'none' }] }]);
      void t.play(1);
    },
    destroy: () => t?.end(),
  };
}
