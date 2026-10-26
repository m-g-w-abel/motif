/**
 * Where a navigation key moves the cursor in a list (`cols` 1) or a grid
 * filled row by row. `page` is the number of rows a PageUp/PageDown moves.
 * Returns null for keys that don't navigate.
 */
export type NavKey = Pick<KeyboardEvent, 'key' | 'altKey' | 'metaKey' | 'ctrlKey'>;

export function navTarget(e: NavKey, index: number, count: number, cols: number, page: number): number | null {
  if (count <= 0 || e.altKey || e.metaKey) return null;
  const last = count - 1;
  const rowStart = index - (index % cols);
  let to: number;
  switch (e.key) {
    case 'ArrowDown':
      to = index + cols;
      break;
    case 'ArrowUp':
      to = index - cols;
      break;
    case 'ArrowRight':
      if (cols === 1) return null;
      to = index + 1;
      break;
    case 'ArrowLeft':
      if (cols === 1) return null;
      to = index - 1;
      break;
    case 'PageDown':
      to = index + page * cols;
      break;
    case 'PageUp':
      to = index - page * cols;
      break;
    // In a grid Home/End go to the row's ends; with Ctrl, to the list's.
    case 'Home':
      to = cols === 1 || e.ctrlKey ? 0 : rowStart;
      break;
    case 'End':
      to = cols === 1 || e.ctrlKey ? last : rowStart + cols - 1;
      break;
    default:
      return null;
  }
  return Math.max(0, Math.min(to, last));
}

/** The scrollTop that brings [top, bottom) into a view of `height`, moving as little as possible. */
export function scrollInto(scrollTop: number, height: number, top: number, bottom: number): number {
  if (top < scrollTop) return top;
  if (bottom > scrollTop + height) return bottom - height;
  return scrollTop;
}
