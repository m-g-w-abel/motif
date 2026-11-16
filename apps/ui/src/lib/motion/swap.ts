import { Transition } from './transition';

/** What `el` holds has just changed: it rises a little into place. */
export const risen = (el: HTMLElement) => new Transition([{ el, frames: [{ opacity: 0.4, transform: 'translateY(12px)' }, { opacity: 1, transform: 'none' }] }]);

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
      t = risen(el);
      void t.play(1);
    },
    destroy: () => t?.end(),
  };
}
