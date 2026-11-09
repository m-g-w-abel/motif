import type { SortRule } from './protocol/SortRule';
import { SORT_KEYS } from './sort.svelte';

/** How a view shows its items: a cover grid, a name list, or a list with covers. */
export type ViewMode = 'grid' | 'list' | 'covers';

export interface ViewOptions {
  mode: ViewMode;
  /** Grid columns; `null` fits as many as the width allows. */
  columns: number | null;
  /** Grid only: titles and artists under the covers. */
  titles: boolean;
  artists: boolean;
  /** Replaces the library's order (Albums); `null` keeps it. */
  sort: SortRule | null;
}

/** What remembers its options: each kind of collection view, and each playlist (`playlist:<id>`). */
export type OptionsKey = 'albums' | 'playlists' | 'played' | 'added' | `playlist:${string}`;

/** Saved options, only those that differ from the defaults. A client setting, per device. */
export type SavedOptions = Record<string, Partial<ViewOptions>>;

export const MAX_COLUMNS = 12;

function defaults(key: OptionsKey): ViewOptions {
  return { mode: key.startsWith('playlist:') ? 'list' : 'grid', columns: null, titles: true, artists: true, sort: null };
}

/** The saved values that make sense; anything else falls back to the default. */
function valid(saved: unknown): Partial<ViewOptions> {
  if (!saved || typeof saved !== 'object') return {};
  const s = saved as Record<string, unknown>;
  const out: Partial<ViewOptions> = {};
  if (s.mode === 'grid' || s.mode === 'list' || s.mode === 'covers') out.mode = s.mode;
  if (Number.isInteger(s.columns) && (s.columns as number) >= 1 && (s.columns as number) <= MAX_COLUMNS) {
    out.columns = s.columns as number;
  }
  if (typeof s.titles === 'boolean') out.titles = s.titles;
  if (typeof s.artists === 'boolean') out.artists = s.artists;
  const sort = s.sort as Record<string, unknown> | null | undefined;
  if (sort && typeof sort.key === 'string' && sort.key in SORT_KEYS && typeof sort.descending === 'boolean') {
    out.sort = { key: sort.key as SortRule['key'], descending: sort.descending };
  }
  return out;
}

export function viewOptions(saved: SavedOptions, key: OptionsKey): ViewOptions {
  return { ...defaults(key), ...valid(saved[key]) };
}

/** `saved` with `patch` applied to `key`, dropping what is back at its default. */
export function withOptions(saved: SavedOptions, key: OptionsKey, patch: Partial<ViewOptions>): SavedOptions {
  const next = { ...viewOptions(saved, key), ...patch };
  const base = defaults(key);
  const own = Object.fromEntries(
    Object.entries(next).filter(([k, v]) => JSON.stringify(v) !== JSON.stringify(base[k as keyof ViewOptions])),
  );
  const { [key]: _, ...rest } = saved;
  return Object.keys(own).length ? { ...rest, [key]: own } : rest;
}
