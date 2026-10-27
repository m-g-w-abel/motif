import type { Column } from './layout/model';

export const DEFAULT_WIDTHS: Record<Column, number> = {
  index: 44,
  title: 300,
  artist: 190,
  album: 210,
  format: 72,
  duration: 64,
};

/** Small columns that keep their width when space runs out. */
const COMPACT = new Set<Column>(['index', 'format', 'duration']);

/** Width the `#` column needs for numbers up to `count`: cell padding plus digits (tabular, ~0.62em at the smallest font) with some slack. */
export const indexWidth = (count: number) => Math.max(44, 20 + 8 * String(Math.max(1, count)).length);

/** The least room the flexible column is given: a quarter of the table, at least 100px. */
export const flexReserve = (tableWidth: number) => Math.max(100, tableWidth * 0.25);

/**
 * Widths to apply to every shown column except `flex`, which absorbs what's
 * left. When the table is too narrow, wide columns shrink proportionally (to
 * no less than 30%) so `flex` keeps `reserved` px; compact columns keep their
 * size. `minIndex` is a floor for the `#` column. A table not measured yet (`tableWidth` 0) gets the requested widths.
 */
export function fitColumns(
  widths: Partial<Record<Column, number>>,
  shown: Column[],
  flex: Column,
  tableWidth: number,
  reserved: number,
  minIndex = 0,
): Partial<Record<Column, number>> {
  const requested = (c: Column) => Math.max(widths[c] ?? DEFAULT_WIDTHS[c], c === 'index' ? minIndex : 0);
  const fixed = shown.filter((c) => c !== flex);
  const compactSum = fixed.filter((c) => COMPACT.has(c)).reduce((n, c) => n + requested(c), 0);
  const wideSum = fixed.filter((c) => !COMPACT.has(c)).reduce((n, c) => n + requested(c), 0);
  const room = tableWidth - reserved - compactSum;
  const scale = tableWidth && wideSum > room ? Math.max(0.3, room / wideSum) : 1;
  return Object.fromEntries(
    fixed.map((c) => [c, Math.round(COMPACT.has(c) ? requested(c) : requested(c) * scale)]),
  ) as Partial<Record<Column, number>>;
}

/**
 * The rows of a windowed list that overlap `from..to` px, as a `[start, end)`
 * range. `tops` holds each row's offset from the top, ascending.
 */
export function visibleRange(tops: number[], from: number, to: number): [number, number] {
  // The last row starting at or before `from`.
  let lo = 0;
  let hi = tops.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid] <= from) lo = mid + 1;
    else hi = mid;
  }
  const start = Math.max(0, lo - 1);
  let end = start;
  while (end < tops.length && tops[end] < to) end++;
  return [start, end];
}

/** The value in sorted `points` closest to `x`, or `x` if there are none. */
export function nearest(points: number[], x: number): number {
  let lo = 0;
  let hi = points.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (points[mid] < x) lo = mid + 1;
    else hi = mid;
  }
  const below = points[lo - 1] ?? -Infinity;
  const above = points[lo] ?? Infinity;
  if (below === -Infinity && above === Infinity) return x;
  return x - below <= above - x ? below : above;
}
