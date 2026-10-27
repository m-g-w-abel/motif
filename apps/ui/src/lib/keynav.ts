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

export interface VimResult {
  /** Typed digits and a pending `g`, to pass back with the next key. */
  pending: string;
  /** The key was a motion or part of one; the caller prevents its default. */
  handled: boolean;
  to: number | null;
}

/**
 * Vim-style navigation. `pending` is the count typed so far, plus `g` after a
 * first `g`. j/k move by rows, h/l by cards in a grid, Ctrl-d/u by half a
 * page, gg/G to the ends; with a count, `5j` moves five and `5G` goes to the
 * fifth row. Any other key drops the pending input.
 */
export function vimTarget(e: NavKey, pending: string, index: number, count: number, cols: number, page: number): VimResult {
  const none = { pending: '', handled: false, to: null };
  if (count <= 0 || e.altKey || e.metaKey) return none;
  const digits = pending.replace('g', '');
  const n = digits ? parseInt(digits, 10) : 0;
  const times = Math.max(1, n);
  const clamp = (to: number) => ({ pending: '', handled: true, to: Math.max(0, Math.min(to, count - 1)) });
  if (e.ctrlKey) {
    const half = Math.max(1, Math.floor(page / 2)) * cols * times;
    return e.key === 'd' ? clamp(index + half) : e.key === 'u' ? clamp(index - half) : none;
  }
  if (/^[0-9]$/.test(e.key) && (e.key !== '0' || digits)) return { pending: digits + e.key, handled: true, to: null };
  switch (e.key) {
    case 'j':
      return clamp(index + times * cols);
    case 'k':
      return clamp(index - times * cols);
    // The search box filters, so the next match is the next item.
    case 'n':
      return clamp(index + times);
    case 'N':
      return clamp(index - times);
    case 'l':
      return cols === 1 ? none : clamp(index + times);
    case 'h':
      return cols === 1 ? none : clamp(index - times);
    case 'G':
      return clamp(n ? (n - 1) * cols : count - 1);
    case 'g':
      return pending.endsWith('g') ? clamp(n ? (n - 1) * cols : 0) : { pending: `${digits}g`, handled: true, to: null };
    default:
      return none;
  }
}
