import type { LayoutNode, PanelNode, Size, SplitNode, Widget } from './model';
import { parseLayout } from './parse';

/**
 * Edits made in place (dragged dividers, reordered widgets), kept per layout
 * and variant on each device; layout files are never rewritten. Keys are node
 * paths such as `split-row/1.split-column/0.artwork`: each step names the node,
 * so a path stops matching where the file changes shape. Added widgets follow
 * a split's file children, so they get the next indices.
 */
export interface Overlay {
  sizes?: Record<string, Size>;
  /** A split's children in the order shown, as indices into the file's order. */
  order?: Record<string, number[]>;
  /** Widgets added to a split, after its file children, by the split's path. */
  added?: Record<string, Widget[]>;
  /** Paths of removed nodes, file or added ones. */
  removed?: string[];
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

const defaults = new Map<Widget, PanelNode>();
/** A widget with the options a layout file gets when it names just the widget. */
function widget(name: Widget): PanelNode {
  let node = defaults.get(name);
  if (!node) {
    const result = parseLayout(`layout version=1\n${name}`);
    if (!result.ok || result.layout.root.kind !== 'panel') throw new Error(`no defaults for ${name}`);
    defaults.set(name, (node = result.layout.root));
  }
  return node;
}

/** A split's file children and then its added widgets. */
export function childrenOf(split: SplitNode, path: string, overlay: Overlay): LayoutNode[] {
  return [...split.children, ...(overlay.added?.[path] ?? []).map(widget)];
}

export interface Slot {
  node: LayoutNode;
  path: string;
  /** Into `childrenOf`. */
  index: number;
  size?: Size;
}

/** A split's children as shown: the overlay's order, sizes, additions and removals over the file's. */
export function slots(split: SplitNode, path: string, overlay: Overlay): Slot[] {
  const children = childrenOf(split, path, overlay);
  return orderOf(overlay, path, children.length).flatMap((index) => {
    const node = children[index];
    const p = childPath(path, index, node);
    return overlay.removed?.includes(p) ? [] : [{ node, path: p, index, size: overlay.sizes?.[p] ?? node.size }];
  });
}

/** Drops edits whose node is gone from `root`, and orders that change nothing. */
export function prune(root: LayoutNode, overlay: Overlay): Overlay {
  const sizes: Record<string, Size> = {};
  const order: Record<string, number[]> = {};
  const added: Record<string, Widget[]> = {};
  const removed: string[] = [];
  const walk = (node: LayoutNode, path: string) => {
    const size = overlay.sizes?.[path];
    if (size) sizes[path] = size;
    if (overlay.removed?.includes(path)) removed.push(path);
    if (node.kind !== 'split') return;
    if (overlay.added?.[path]?.length) added[path] = overlay.added[path];
    const children = childrenOf(node, path, overlay);
    const shown = orderOf(overlay, path, children.length);
    if (shown.some((v, i) => v !== i)) order[path] = shown;
    children.forEach((child, i) => walk(child, childPath(path, i, child)));
  };
  walk(root, rootPath(root));
  return {
    ...(Object.keys(sizes).length ? { sizes } : {}),
    ...(Object.keys(order).length ? { order } : {}),
    ...(Object.keys(added).length ? { added } : {}),
    ...(removed.length ? { removed } : {}),
  };
}

/** `node` as shown: the overlay's edits written into the tree. Splits left empty are dropped, as a file can't have them. */
export function applyOverlay(node: LayoutNode, overlay: Overlay, path = rootPath(node)): LayoutNode {
  if (node.kind !== 'split') return node;
  const children = slots(node, path, overlay)
    .map((s) => ({ ...applyOverlay(s.node, overlay, s.path), size: s.size }))
    .filter((child) => child.kind !== 'split' || child.children.length);
  return { ...node, children };
}
