/** A copy of `list` with the item at `from` moved to `to`. */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const next = [...list];
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}

/** Where a dragged row (from index `from`, centre at `y`) would land, given the other rows' centres. */
export function dropIndex(mids: readonly number[], from: number, y: number): number {
  return mids.filter((m, i) => i !== from && m < y).length;
}

/** Which way row `i` shifts (-1 up, 1 down, 0 stays) while row `from` is dragged to `to`. */
export function shift(i: number, from: number, to: number): -1 | 0 | 1 {
  if (from < to && i > from && i <= to) return -1;
  if (to < from && i >= to && i < from) return 1;
  return 0;
}

/** The Alt+Arrow target for a key, or null when it isn't a reorder key or would leave the list. */
export function keyTarget(e: { key: string; altKey: boolean }, from: number, count: number): number | null {
  if (!e.altKey) return null;
  const to = e.key === 'ArrowUp' ? from - 1 : e.key === 'ArrowDown' ? from + 1 : -1;
  return to >= 0 && to < count ? to : null;
}
