/**
 * Multi-selection in lists and grids: the picked items by key, each with the
 * index it was picked at so actions can run in list order. Replaced on every
 * change, never mutated, so it can live in reactive state.
 */
export type Selection<T> = Map<string, { index: number; item: T }>;

/** Item `i` as [key, item], or undefined while it loads. */
export type ItemAt<T> = (i: number) => [string, T] | undefined;

/** Items `from` to `to`, inclusive, in either order. */
export function span<T>(at: ItemAt<T>, from: number, to: number): Selection<T> {
  const picked: Selection<T> = new Map();
  for (let i = Math.min(from, to); i <= Math.max(from, to); i++) {
    const it = at(i);
    if (it) picked.set(it[0], { index: i, item: it[1] });
  }
  return picked;
}

/** Ctrl-click: adds item `i` or takes it out. */
export function toggle<T>(picked: Selection<T>, at: ItemAt<T>, i: number): Selection<T> {
  const it = at(i);
  if (!it) return picked;
  const next = new Map(picked);
  if (!next.delete(it[0])) next.set(it[0], { index: i, item: it[1] });
  return next;
}

/** The picked items in list order. */
export function ordered<T>(picked: Selection<T>): T[] {
  return [...picked.values()].sort((a, b) => a.index - b.index).map((p) => p.item);
}

/** Whether an action on item `key` applies to the whole selection rather than the one item. */
export const inSelection = (picked: Selection<unknown>, key: string) => picked.size > 1 && picked.has(key);
