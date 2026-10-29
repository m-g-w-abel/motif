import type { LayoutNode, PanelNode, Size, Widget } from './model';
import { parseLayout } from './parse';

/**
 * Edits made in place (dragged dividers, moved, added and removed widgets),
 * kept per layout and variant on each device; layout files are never
 * rewritten. The device keeps the edited tree, with the file's tree it was
 * made from, so edits are dropped once the file changes.
 */
export interface Overlay {
  /** `fingerprint` of the file's tree. */
  of: string;
  root: LayoutNode;
}

export const fingerprint = (root: LayoutNode) => JSON.stringify(root);

/** The tree on screen: the edited one if it was made from this `root`. */
export const shownRoot = (root: LayoutNode, overlay: Overlay | undefined) =>
  overlay?.of === fingerprint(root) ? overlay.root : root;

/** A node's place in the tree: child indices from the root. */
export type At = number[];
export type Side = 'left' | 'right' | 'above' | 'below';

const name = (node: LayoutNode) => (node.kind === 'split' ? `split-${node.direction}` : node.panel);
/** Names a node by its place, such as `split-row/1.split-column/0.artwork`, to key a widget's remembered state. */
export const rootPath = name;
export const childPath = (path: string, i: number, child: LayoutNode) => `${path}/${i}.${name(child)}`;

const ids = new WeakMap<LayoutNode, number>();
let nextId = 0;
/** Stays the same when an edit rebuilds a node, so its widget keeps its state. */
export function idOf(node: LayoutNode): number {
  let id = ids.get(node);
  if (id === undefined) ids.set(node, (id = nextId++));
  return id;
}

function rebuilt<T extends LayoutNode>(old: LayoutNode, node: T): T {
  ids.set(node, idOf(old));
  return node;
}

const defaults = new Map<Widget, PanelNode>();
/** A widget with the options a layout file gets when it names just the widget. */
export function widget(name: Widget): PanelNode {
  let node = defaults.get(name);
  if (!node) {
    const result = parseLayout(`layout version=1\n${name}`);
    if (!result.ok || result.layout.root.kind !== 'panel') throw new Error(`no defaults for ${name}`);
    defaults.set(name, (node = result.layout.root));
  }
  return { ...node };
}

export function nodeAt(root: LayoutNode, at: At): LayoutNode | undefined {
  let node: LayoutNode | undefined = root;
  for (const i of at) node = node?.kind === 'split' ? node.children[i] : undefined;
  return node;
}

/** Replaces the node at `at` with what `edit` returns; a split's child list may grow or shrink. */
function editAt(root: LayoutNode, at: At, edit: (node: LayoutNode) => LayoutNode[]): LayoutNode[] {
  if (!at.length) return edit(root);
  if (root.kind !== 'split' || !root.children[at[0]]) return [root];
  const [i, ...rest] = at;
  const children = root.children.flatMap((child, j) => (j === i ? editAt(child, rest, edit) : [child]));
  // A split left with one widget becomes that widget, in the split's place.
  if (children.length === 1 && root.children.length > 1) return [rebuilt(children[0], { ...children[0], size: root.size })];
  return [rebuilt(root, { ...root, children })];
}

const one = (nodes: LayoutNode[], fallback: LayoutNode) => (nodes.length === 1 ? nodes[0] : fallback);

/** Sets or (with no size) clears the size of the node at `at`. */
export function resize(root: LayoutNode, at: At, size: Size | undefined): LayoutNode {
  return one(
    editAt(root, at, (node) => [rebuilt(node, { ...node, size })]),
    root,
  );
}

/** Removes the node at `at`. The root stays: there must be something to show. */
export function remove(root: LayoutNode, at: At): LayoutNode {
  return at.length ? one(editAt(root, at, () => []), root) : root;
}

/**
 * Puts `node` beside the node at `at`. A split running that way takes it in
 * (the parent split, or the target itself); otherwise the target is wrapped
 * in a new split.
 */
export function insert(root: LayoutNode, at: At, side: Side, node: LayoutNode): LayoutNode {
  const direction = side === 'left' || side === 'right' ? 'row' : 'column';
  const after = side === 'right' || side === 'below';
  const target = nodeAt(root, at);
  if (!target) return root;
  if (target.kind === 'split' && target.direction === direction) {
    const children = after ? [...target.children, node] : [node, ...target.children];
    return one(
      editAt(root, at, () => [rebuilt(target, { ...target, children })]),
      root,
    );
  }
  const parent = nodeAt(root, at.slice(0, -1));
  if (at.length && parent?.kind === 'split' && parent.direction === direction) {
    const i = at[at.length - 1] + (after ? 1 : 0);
    const children = [...parent.children.slice(0, i), node, ...parent.children.slice(i)];
    return one(
      editAt(root, at.slice(0, -1), () => [rebuilt(parent, { ...parent, children })]),
      root,
    );
  }
  return one(
    editAt(root, at, (old) => {
      const inner = rebuilt(old, { ...old, size: undefined });
      return [{ kind: 'split', direction, size: old.size, children: after ? [inner, node] : [node, inner] }];
    }),
    root,
  );
}

/** Moves the node at `from` beside the one at `to`. Nothing happens if `to` is inside `from`. */
export function move(root: LayoutNode, from: At, to: At, side: Side): LayoutNode {
  const node = nodeAt(root, from);
  if (!node || from.every((v, i) => to[i] === v)) return root;
  // Put a copy in first, so `to` still points at the target, then take out the original.
  const placed = insert(root, to, side, rebuilt(node, { ...node }));
  const left = find(placed, node);
  return left ? remove(placed, left) : root;
}

/** Where `node` (that very object) is in the tree. */
function find(root: LayoutNode, node: LayoutNode, at: At = []): At | null {
  if (root === node) return at;
  if (root.kind !== 'split') return null;
  for (const [i, child] of root.children.entries()) {
    const found = find(child, node, [...at, i]);
    if (found) return found;
  }
  return null;
}
