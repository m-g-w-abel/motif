import type { ViewState } from './context';
import { SOURCES, type Source } from './layout/store.svelte';
import type { SortKey } from './protocol/SortKey';
import type { SortRule } from './protocol/SortRule';

/** Every key a list can be sorted by, with its name in the sort menu. */
export const SORT_KEYS: Record<SortKey, string> = {
  album_artist: 'Album artist',
  artist: 'Artist',
  album: 'Album',
  year: 'Year',
  added: 'Date added',
  title: 'Title',
  genre: 'Genre',
  disc: 'Disc',
  track: 'Track number',
  duration: 'Duration',
  format: 'Format',
  path: 'File path',
};

/** The library's own order (`db.rs`): album artist, album, disc, track. */
export const DEFAULT_RULES: readonly SortRule[] = [
  { key: 'album_artist', descending: false },
  { key: 'album', descending: false },
  { key: 'disc', descending: false },
  { key: 'track', descending: false },
];

const storageKey = (source: Source) => `sort.${source}`;

/** Saved rules, or the default if there are none or they don't make sense. */
function load(source: Source): SortRule[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(storageKey(source)) ?? 'null');
    const rules = Array.isArray(saved)
      ? saved
          .filter((r) => r && r.key in SORT_KEYS && typeof r.descending === 'boolean')
          .map((r) => ({
            key: r.key as SortKey,
            descending: r.descending as boolean,
          }))
      : [];
    if (rules.length) return rules;
  } catch {
    // Defaults.
  }
  return [...DEFAULT_RULES];
}

/** Each view's tiered sort rules. A per-device preference, like the layout choice. */
export class SortStore {
  rules = $state(Object.fromEntries(SOURCES.map((s) => [s, load(s)])) as Record<Source, SortRule[]>);

  /** Saves `rules` for `source`; none means the default order. */
  set(source: Source, rules: SortRule[]) {
    this.rules[source] = rules.length ? rules : [...DEFAULT_RULES];
    try {
      if (isDefault(this.rules[source])) localStorage.removeItem(storageKey(source));
      else localStorage.setItem(storageKey(source), JSON.stringify(this.rules[source]));
    } catch {
      // Applies until reload.
    }
  }
}

export const isDefault = (rules: readonly SortRule[]) =>
  rules.length === DEFAULT_RULES.length &&
  rules.every((r, i) => r.key === DEFAULT_RULES[i].key && r.descending === DEFAULT_RULES[i].descending);

export const sortRules = new SortStore();

/**
 * A column header click: sorts by that column, then reverses it, then
 * (`null`) goes back to the saved rules. It never changes the saved rules.
 */
export function nextOneOff(current: SortRule | null, key: SortKey): SortRule | null {
  if (current?.key !== key) return { key, descending: false };
  return current.descending ? null : { key, descending: true };
}

/** The order a list uses: a one-off column sort first, with the saved rules breaking ties. */
export function effectiveSort(rules: readonly SortRule[], oneOff: SortRule | null): SortRule[] {
  return oneOff ? [oneOff, ...rules.filter((r) => r.key !== oneOff.key)] : [...rules];
}

/** The order a view's lists use; every list and play request of the view must agree. */
export function viewSort(view: ViewState, store: SortStore = sortRules): SortRule[] {
  return effectiveSort(store.rules[view.source], view.browse.oneOff);
}
