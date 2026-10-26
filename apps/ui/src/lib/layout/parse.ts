import { InvalidKdlError, getLocation, parse, type Node, type Primitive } from '@bgotink/kdl';
import {
  COLUMNS,
  DEFAULT_CARD_SIZE,
  DEFAULT_COLUMNS,
  INFO_STYLES,
  LAYOUT_ICONS,
  LAYOUT_VERSION,
  TRACKLIST_STYLES,
  type Column,
  type Layout,
  type LayoutNode,
  type Size,
} from './model';

export interface LayoutError {
  message: string;
  /** 1-based; null when the error isn't tied to one place in the file. */
  line: number | null;
  column: number | null;
}

export type ParseResult = { ok: true; layout: Layout } | { ok: false; errors: LayoutError[] };

const CONTENT_NODES = ['split', 'tracklist', 'album-grid', 'artwork', 'track-info', 'controls', 'queue'];

/**
 * Parses and validates a layout file, collecting every error it finds.
 * `fallbackName` names layouts whose `layout` node has no `name`, e.g. the
 * file name.
 */
export function parseLayout(text: string, fallbackName = 'Untitled'): ParseResult {
  let nodes: Node[];
  try {
    nodes = parse(text, { storeLocations: true }).nodes;
  } catch (e) {
    if (!(e instanceof InvalidKdlError)) throw e;
    const errors = e.errors?.length ? e.errors : [e];
    return {
      ok: false,
      errors: errors.map((err) => ({
        message: err.message,
        line: err.start?.line ?? null,
        column: err.start?.column ?? null,
      })),
    };
  }

  const reader = new Reader();
  const layout = reader.layout(nodes, fallbackName);
  return reader.errors.length || !layout ? { ok: false, errors: reader.errors } : { ok: true, layout };
}

const list = (items: readonly string[]) => items.map((i) => `\`${i}\``).join(', ');

class Reader {
  errors: LayoutError[] = [];

  error(node: Node | undefined, message: string) {
    const start = node && getLocation(node)?.start;
    this.errors.push({ message, line: start?.line ?? null, column: start?.column ?? null });
  }

  layout(nodes: Node[], fallbackName: string): Layout | null {
    const [header, ...rest] = nodes;
    if (header?.getName() !== 'layout') {
      this.error(header, `the file must start with \`layout version=${LAYOUT_VERSION} name="…"\``);
      return null;
    }
    this.check(header, ['version', 'name', 'description', 'icon']);
    const version = header.getProperty('version');
    if (version !== LAYOUT_VERSION) {
      this.error(header, `unsupported layout version ${String(version)}; this app reads version ${LAYOUT_VERSION}`);
    }

    const name = this.string(header, 'name', fallbackName);
    const description = this.string(header, 'description', '');
    const icon = this.oneOf(header, 'icon', LAYOUT_ICONS, 'list');

    const content = rest.flatMap((node) => {
      if (node.getName() === 'layout') {
        this.error(node, '`layout` can only appear once, at the top');
        return [];
      }
      return this.contentNode(node) ?? [];
    });
    if (rest.length === 0) this.error(header, 'the layout is empty; add a widget such as `tracklist`');
    if (content.length === 0) return null;

    return {
      version: LAYOUT_VERSION,
      name,
      description,
      icon,
      // Several top-level widgets stack top to bottom.
      root: content.length === 1 ? content[0] : { kind: 'split', direction: 'column', children: content },
    };
  }

  /** The content nodes among `node`'s children. */
  content(node: Node): LayoutNode[] {
    return (node.children?.nodes ?? []).flatMap((child) => this.contentNode(child) ?? []);
  }

  contentNode(node: Node): LayoutNode | null {
    const name = node.getName();
    switch (name) {
      case 'split': {
        this.check(node, ['size'], 1, true);
        const direction = node.getArgument(0);
        if (direction !== 'row' && direction !== 'column') {
          this.error(node, 'a split needs a direction: `split row { ... }` or `split column { ... }`');
          return null;
        }
        const children = this.content(node);
        if (!node.children?.nodes.length) this.error(node, 'this split is empty');
        return { kind: 'split', direction, size: this.size(node), children };
      }
      case 'tracklist': {
        this.check(node, ['style', 'max-width', 'size'], 0, true);
        const style = this.oneOf(node, 'style', TRACKLIST_STYLES, 'table');
        return {
          kind: 'panel',
          panel: 'tracklist',
          size: this.size(node),
          style,
          columns: this.columns(node, DEFAULT_COLUMNS[style]),
          maxWidth: this.pixels(node, 'max-width', 400, 4000),
        };
      }
      case 'album-grid':
        this.check(node, ['card-size', 'columns', 'size']);
        return {
          kind: 'panel',
          panel: 'album-grid',
          size: this.size(node),
          cardSize: this.pixels(node, 'card-size', 80, 600) ?? DEFAULT_CARD_SIZE,
          columns: this.count(node, 'columns', 1, 24),
        };
      case 'track-info':
        this.check(node, ['style', 'size']);
        return {
          kind: 'panel',
          panel: 'track-info',
          size: this.size(node),
          style: this.oneOf(node, 'style', INFO_STYLES, 'list'),
        };
      case 'queue': {
        this.check(node, ['collapsible', 'size']);
        const collapsible = node.getProperty('collapsible');
        if (collapsible !== undefined && typeof collapsible !== 'boolean') {
          this.error(node, `\`collapsible\` must be #true or #false, got ${fmt(collapsible)}`);
        }
        return { kind: 'panel', panel: 'queue', size: this.size(node), collapsible: collapsible === true };
      }
      case 'artwork':
      case 'controls':
        this.check(node, ['size']);
        return { kind: 'panel', panel: name, size: this.size(node) };
      default:
        this.error(node, `unknown widget \`${name}\`; expected one of ${list(CONTENT_NODES)}`);
        return null;
    }
  }

  columns(node: Node, defaults: Column[]): Column[] {
    const children = node.children?.nodes ?? [];
    for (const child of children) {
      if (child.getName() !== 'columns') {
        this.error(child, `unexpected \`${child.getName()}\` inside \`tracklist\`; only \`columns\` is allowed`);
      }
    }
    const columnsNode = children.find((c) => c.getName() === 'columns');
    if (!columnsNode) return defaults;
    this.check(columnsNode, [], Infinity);
    const columns = columnsNode.getArguments().filter((c): c is Column => {
      const known = typeof c === 'string' && (COLUMNS as readonly string[]).includes(c);
      if (!known) this.error(columnsNode, `unknown column \`${String(c)}\`; expected one of ${list(COLUMNS)}`);
      return known;
    });
    if (columns.length === 0) this.error(columnsNode, '`columns` needs at least one column');
    return columns;
  }

  size(node: Node): Size | undefined {
    const value = node.getProperty('size');
    if (value === undefined) return undefined;
    if (typeof value === 'number' && value > 0) return { px: value };
    if (typeof value === 'string') {
      const match = /^(\d+(?:\.\d+)?)%$/.exec(value);
      const percent = match ? Number(match[1]) : NaN;
      if (percent > 0 && percent <= 100) return { percent };
    }
    this.error(node, `invalid size ${fmt(value)}; use pixels (\`size=240\`) or a percentage (\`size="30%"\`)`);
    return undefined;
  }

  pixels(node: Node, prop: string, min: number, max: number): number | undefined {
    const value = node.getProperty(prop);
    if (value === undefined) return undefined;
    if (typeof value === 'number' && value >= min && value <= max) return value;
    this.error(node, `\`${prop}\` must be a number of pixels between ${min} and ${max}, got ${fmt(value)}`);
    return undefined;
  }

  count(node: Node, prop: string, min: number, max: number): number | undefined {
    const value = node.getProperty(prop);
    if (value === undefined) return undefined;
    if (typeof value === 'number' && Number.isInteger(value) && value >= min && value <= max) return value;
    this.error(node, `\`${prop}\` must be a whole number between ${min} and ${max}, got ${fmt(value)}`);
    return undefined;
  }

  string(node: Node, prop: string, fallback: string): string {
    const value = node.getProperty(prop);
    if (value === undefined) return fallback;
    if (typeof value === 'string') return value;
    this.error(node, `\`${prop}\` must be text, got ${fmt(value)}`);
    return fallback;
  }

  oneOf<T extends string>(node: Node, prop: string, allowed: readonly T[], fallback: T): T {
    const value = node.getProperty(prop);
    if (value === undefined) return fallback;
    if (typeof value === 'string' && (allowed as readonly string[]).includes(value)) return value as T;
    this.error(node, `\`${prop}\` must be one of ${list(allowed)}, got ${fmt(value)}`);
    return fallback;
  }

  /** Rejects unknown properties, extra arguments and unexpected children. */
  check(node: Node, props: string[], maxArgs = 0, children = false) {
    const name = node.getName();
    for (const prop of node.getProperties().keys()) {
      if (!props.includes(prop)) {
        const expected = props.length ? `; expected ${list(props)}` : '';
        this.error(node, `\`${name}\` has no property \`${prop}\`${expected}`);
      }
    }
    if (node.getArguments().length > maxArgs) {
      this.error(node, `\`${name}\` takes ${maxArgs === 0 ? 'no' : `at most ${maxArgs}`} argument${maxArgs === 1 ? '' : 's'}`);
    }
    if (!children && node.children?.nodes.length) {
      this.error(node, `\`${name}\` can't have children`);
    }
  }
}

function fmt(value: Primitive): string {
  return typeof value === 'string' ? `"${value}"` : String(value);
}
