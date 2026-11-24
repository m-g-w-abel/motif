/** Pixels beyond the view whose cards are drawn (and their covers loaded), so a fling shows no blank ones. */
export const GRID_OVERSCAN = 600;

/** The rows `[first, last)` of a grid of `rows` rows of `rowHeight` px to draw for a view `top` px into the cards. */
export function gridRows(top: number, viewHeight: number, rowHeight: number, rows: number, overscan = GRID_OVERSCAN) {
  return {
    first: Math.max(0, Math.floor((top - overscan) / rowHeight)),
    last: Math.min(rows, Math.ceil((top + viewHeight + overscan) / rowHeight)),
  };
}
