import { Document, Node, format } from '@bgotink/kdl';
import { DEFAULT_CARD_SIZE, DEFAULT_COLUMNS, STYLE_OPTIONS, type Layout, type LayoutNode, type Size } from './model';

/** Writes a layout as a layout file that `parseLayout` reads back to the same layout. Comments aren't kept. */
export function serializeLayout(layout: Layout): string {
  const doc = new Document();
  const header = Node.create('layout');
  header.setProperty('version', layout.version);
  header.setProperty('name', layout.name);
  if (layout.description) header.setProperty('description', layout.description);
  header.setProperty('icon', layout.icon);
  doc.appendNode(header);
  appendContent(doc, layout.root);
  if (layout.phone) {
    const when = Node.create('when');
    when.addArgument('phone');
    appendContent(when, layout.phone);
    doc.appendNode(when);
  }
  return format(doc);
}

// A top-level column split is how the parser reads several top-level widgets, so it's written back that way.
function appendContent(parent: Document | Node, root: LayoutNode) {
  const nodes =
    root.kind === 'split' && root.direction === 'column' && !root.size && root.children.length > 1
      ? root.children
      : [root];
  for (const node of nodes) parent.appendNode(toNode(node));
}

function toNode(node: LayoutNode): Node {
  const out = Node.create(node.kind === 'split' ? 'split' : node.panel);
  if (node.kind === 'split') {
    out.addArgument(node.direction);
    for (const child of node.children) out.appendNode(toNode(child));
  } else if (node.panel === 'tracklist') {
    const style = node.albumBreaks ? 'grouped' : 'table';
    if (style !== 'table') out.setProperty('style', style);
    if (node.headers !== STYLE_OPTIONS[style].headers) out.setProperty('headers', node.headers);
    if (node.maxWidth !== undefined) out.setProperty('max-width', node.maxWidth);
    if (node.columns.join() !== DEFAULT_COLUMNS[style].join()) {
      const columns = Node.create('columns');
      for (const column of node.columns) columns.addArgument(column);
      out.appendNode(columns);
    }
  } else if (node.panel === 'album-grid') {
    if (node.cardSize !== DEFAULT_CARD_SIZE) out.setProperty('card-size', node.cardSize);
    if (node.columns !== undefined) out.setProperty('columns', node.columns);
  } else if (node.panel === 'track-info') {
    if (node.style !== 'list') out.setProperty('style', node.style);
  } else if (node.panel === 'queue') {
    if (node.collapsible) out.setProperty('collapsible', true);
  }
  if (node.size) out.setProperty('size', sizeValue(node.size));
  return out;
}

const sizeValue = (size: Size) => ('px' in size ? size.px : `${size.percent}%`);
