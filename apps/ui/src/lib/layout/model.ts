/**
 * Typed form of a layout file. A layout arranges the widgets of the music
 * area only; navigation, the player bar and theme live outside layouts. See
 * docs/LAYOUT.md for the file format.
 */

export const LAYOUT_VERSION = 1;

/** Icons a layout can show on its button. */
export const LAYOUT_ICONS = ['list', 'grid', 'split', 'info', 'columns'] as const;
export type LayoutIcon = (typeof LAYOUT_ICONS)[number];

export const COLUMNS = ['index', 'title', 'artist', 'album', 'format', 'duration'] as const;
export type Column = (typeof COLUMNS)[number];

/** `table`: one row per track with column headers. `grouped`: tracks under album headers. */
export const TRACKLIST_STYLES = ['table', 'grouped'] as const;
export type TracklistStyle = (typeof TRACKLIST_STYLES)[number];

/** `list`: title, artist and details stacked. `table`: a field/value table. `hero`: large type. */
export const INFO_STYLES = ['list', 'table', 'hero'] as const;
export type InfoStyle = (typeof INFO_STYLES)[number];

/** Fixed pixels, or a share of the parent split. */
export type Size = { px: number } | { percent: number };

export interface SplitNode {
  kind: 'split';
  direction: 'row' | 'column';
  size?: Size;
  children: LayoutNode[];
}

export type PanelNode = {
  kind: 'panel';
  size?: Size;
} & (
  | { panel: 'tracklist'; albumBreaks: boolean; headers: boolean; columns: Column[]; maxWidth?: number }
  | { panel: 'album-grid'; cardSize: number; columns?: number }
  | { panel: 'artwork' }
  | { panel: 'track-info'; style: InfoStyle }
  | { panel: 'controls' }
  | { panel: 'queue'; collapsible: boolean }
  | { panel: 'visualiser' }
);

export type LayoutNode = SplitNode | PanelNode;

export type Widget = PanelNode['panel'];

/** Every widget, by its name in a layout file, with the name shown while arranging. */
export const WIDGETS: Record<Widget, string> = {
  tracklist: 'Track list',
  'album-grid': 'Album grid',
  artwork: 'Artwork',
  'track-info': 'Track info',
  controls: 'Controls',
  queue: 'Queue',
  visualiser: 'Visualiser',
};

export interface Layout {
  version: number;
  /** Shown on the layout's button. */
  name: string;
  description: string;
  icon: LayoutIcon;
  root: LayoutNode;
  /** The `when phone { ... }` variant, used at 700px wide or less instead of `root`. */
  phone?: LayoutNode;
}

export const DEFAULT_COLUMNS: Record<TracklistStyle, Column[]> = {
  table: ['index', 'title', 'artist', 'album', 'duration'],
  grouped: ['index', 'artist', 'title', 'duration'],
};

/** What the `style` shorthand of `tracklist` stands for. */
export const STYLE_OPTIONS: Record<TracklistStyle, { albumBreaks: boolean; headers: boolean }> = {
  table: { albumBreaks: false, headers: true },
  grouped: { albumBreaks: true, headers: false },
};

export const DEFAULT_CARD_SIZE = 180;
