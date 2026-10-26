import type { LayoutNode, Size, SplitNode } from './model';

/**
 * Edits made in place (dragged dividers, reordered widgets), kept per layout
 * and variant on each device; layout files are never rewritten. Keys are node
 * paths such as `split-row/1.split-column/0.artwork`: each step names the node,
 * so a path stops matching where the file changes shape.
 */
export interface Overlay {
  sizes?: Record<string, Size>;
  /** A split's children in the order shown, as indices into the file's order. */
  order?: Record<string, number[]>;
}

const name = (node: LayoutNode) => (node.kind === 'split' ? `split-${node.direction}` : node.panel);
export const rootPath = name;
const childPath = (path: string, i: number, child: LayoutNode) => `${path}/${i}.${name(child)}`;

const isOrder = (order: number[] | undefined, n: number): order is number[] =>
  Array.isArray(order) && order.length === n && [...Array(n).keys()].every((i) => order.includes(i));

/** The order a split at `path` with `n` children is shown in. */
export function orderOf(overlay: Overlay, path: string, n: number): number[] {
  const order = overlay.order?.[path];
  return isOrder(order, n) ? order : [...Array(n).keys()];
}

export interface Slot {
  node: LayoutNode;
  path: string;
  size?: Size;
}

/** A split's children as shown: the overlay's order and sizes over the file's. */
export function slots(split: SplitNode, path: string, overlay: Overlay): Slot[] {
  return orderOf(overlay, path, split.children.length).map((i) => {
    const node = split.children[i];
    const p = childPath(path, i, node);
    return { node, path: p, size: overlay.sizes?.[p] ?? node.size };
  });
}

/** Drops edits whose node is gone from `root`, and orders that change nothing. */
export function prune(root: LayoutNode, overlay: Overlay): Overlay {
  const sizes: Record<string, Size> = {};
  const order: Record<string, number[]> = {};
  const walk = (node: LayoutNode, path: string) => {
    const size = overlay.sizes?.[path];
    if (size) sizes[path] = size;
    if (node.kind !== 'split') return;
    const shown = orderOf(overlay, path, node.children.length);
    if (shown.some((v, i) => v !== i)) order[path] = shown;
    node.children.forEach((child, i) => walk(child, childPath(path, i, child)));
  };
  walk(root, rootPath(root));
  return {
    ...(Object.keys(sizes).length ? { sizes } : {}),
    ...(Object.keys(order).length ? { order } : {}),
  };
}
