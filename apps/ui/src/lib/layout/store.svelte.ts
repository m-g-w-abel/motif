import { format, parse } from '@bgotink/kdl';
import { MediaQuery } from 'svelte/reactivity';
import type { Layout, LayoutNode, Size, Widget } from './model';
import * as edit from './overlay';
import { fingerprint, shownRoot, type At, type Overlay, type Side } from './overlay';
import { serializeLayout } from './serialize';
import { parseLayout, type LayoutError } from './parse';
import type { SortRule } from '../protocol/SortRule';
import type { TrackRef } from '../protocol/TrackRef';
import type { Selection } from '../selection';
import { builtinLayouts, type LayoutSource } from './presets';

// Layouts are a client-side concern: user layouts and the choice of layout
// live in this device's storage, never in the engine.
const USER_KEY = 'layouts.user';
const SOURCE_KEY = 'browse.source';
const activeKey = (source: Source) => `layouts.active.${source}`;
const editsKey = (id: string, variant: Variant) => `layouts.edits.${id}.${variant}`;

/** Which tree of a layout is showing: its `when phone` one at 700px or less, else the main one. */
export type Variant = 'desktop' | 'phone';
const VARIANTS: Variant[] = ['desktop', 'phone'];
const rootOf = (layout: Layout, variant: Variant): LayoutNode | undefined =>
  variant === 'phone' ? layout.phone : layout.root;

/** What the music area is showing; picked in the left bar. Saved playlists join this in phase 1. */
export type Source = 'library';
export const SOURCES: Source[] = ['library'];

/**
 * Widget state that should survive switching layouts: the same selection and
 * album focus carry over whichever widgets are showing them.
 */
export interface BrowseState {
  /** The last clicked row. */
  selected: string | null;
  /** The last clicked row's track, for widgets (the info pane) that show it. */
  selectedTrack: TrackRef | null;
  /** Rows picked in a track list; usually just `selected`, more with Ctrl/Shift-click. */
  picked: Selection<TrackRef>;
  /** The last clicked album card. */
  focusAlbum: string | null;
  /** Album cards picked in a grid, by key. */
  pickedAlbums: Selection<string>;
  /** What the library is filtered to; empty for everything. */
  search: string;
  /** A column header's sort, used instead of the saved rules until clicked away; never saved. */
  oneOff: SortRule | null;
}

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Edits stored by an older version, in another shape, are dropped. */
function loadEdits(key: string): Overlay | undefined {
  try {
    const overlay = JSON.parse(load(key) ?? 'null');
    return typeof overlay?.of === 'string' && overlay.root ? overlay : undefined;
  } catch {
    return undefined;
  }
}

function save(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode); changes then last until reload.
  }
}

/** A layout file that parsed, ready to show in the picker. */
export interface LayoutEntry extends LayoutSource {
  layout: Layout;
}

const stem = (fileName: string) => fileName.replace(/\.kdl$/i, '');

function toEntry(source: LayoutSource): LayoutEntry | null {
  const result = parseLayout(source.text, stem(source.fileName));
  return result.ok ? { ...source, layout: result.layout } : null;
}

/** A safe file name for a layout name: `My Layout!` → `my-layout.kdl`. */
function fileNameFor(name: string): string {
  const slug = name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
  return `${slug || 'layout'}.kdl`;
}

export type InstallResult = { ok: true; entry: LayoutEntry } | { ok: false; errors: LayoutError[] };

export class LayoutStore {
  builtins: LayoutEntry[] = builtinLayouts.map((source) => {
    const entry = toEntry(source);
    if (!entry) throw new Error(`built-in layout ${source.fileName} is invalid`);
    return entry;
  });

  user = $state.raw<LayoutEntry[]>([]);
  source = $state<Source>('library');
  /** Each source remembers its own layout, e.g. a gallery for the library. */
  activeBySource = $state<Record<Source, string>>({ library: builtinLayouts[0].id });
  #browse = new Map<string, BrowseState>();

  all: LayoutEntry[] = $derived([...this.builtins, ...this.user]);
  activeId: string = $derived(this.activeBySource[this.source]);
  active: LayoutEntry = $derived(this.all.find((e) => e.id === this.activeId) ?? this.builtins[0]);
  layout: Layout = $derived(this.active.layout);

  // No window under unit tests.
  #narrow = typeof window === 'undefined' ? { current: false } : new MediaQuery('(max-width: 700px)');
  variant: Variant = $derived(this.#narrow.current && this.layout.phone ? 'phone' : 'desktop');
  /** The tree on screen, before in-place edits. */
  root: LayoutNode = $derived(rootOf(this.layout, this.variant) ?? this.layout.root);
  /** In-place edits written this session, by storage key (`null` once dropped); the rest are read from storage. */
  #edits = $state.raw<Record<string, Overlay | null>>({});
  #editsKey = $derived(editsKey(this.active.id, this.variant));
  #overlay = $derived(this.#overlayAt(this.#editsKey));
  /** The tree on screen, with this device's in-place edits. */
  shown: LayoutNode = $derived(shownRoot(this.root, this.#overlay));
  edited = $derived(this.shown !== this.root);
  /** Edit mode: widgets show handles to move, add and remove them. */
  arranging = $state(false);

  constructor() {
    try {
      const stored: LayoutSource[] = JSON.parse(load(USER_KEY) ?? '[]');
      // A stored layout that no longer parses (e.g. after an app update) is
      // dropped from the picker rather than breaking the UI.
      this.user = stored.flatMap((s) => toEntry({ ...s, builtin: false }) ?? []);
    } catch {
      this.user = [];
    }
    for (const source of SOURCES) {
      const id = load(activeKey(source));
      if (id && this.all.some((e) => e.id === id)) this.activeBySource[source] = id;
    }
    const source = load(SOURCE_KEY);
    if ((SOURCES as string[]).includes(source ?? '')) this.source = source as Source;
  }

  /** Shared, long-lived widget state for a source (or another area such as the now-playing screen). */
  browseState(key: string): BrowseState {
    const existing = this.#browse.get(key);
    if (existing) return existing;
    const created: BrowseState = $state({
      selected: null,
      selectedTrack: null,
      picked: new Map(),
      focusAlbum: null,
      pickedAlbums: new Map(),
      search: '',
      oneOff: null,
    });
    this.#browse.set(key, created);
    return created;
  }

  /** Uses a layout for the current source. */
  select(id: string) {
    const entry = this.all.find((e) => e.id === id) ?? this.builtins[0];
    this.activeBySource[this.source] = entry.id;
    save(activeKey(this.source), entry.id);
  }

  browse(source: Source) {
    this.source = source;
    save(SOURCE_KEY, source);
  }

  /**
   * Installs or replaces a user layout from file contents. A file with the
   * same name as an installed one replaces it.
   */
  install(text: string, fileName: string): InstallResult {
    const result = parseLayout(text, stem(fileName));
    if (!result.ok) return result;
    const name = fileNameFor(result.layout.name);
    const entry: LayoutEntry = { id: `user:${name}`, fileName: name, text, builtin: false, layout: result.layout };
    this.user = [...this.user.filter((e) => e.id !== entry.id), entry];
    this.persist();
    this.#pruneEdits(entry.id, entry.layout);
    this.select(entry.id);
    return { ok: true, entry };
  }

  /**
   * Saves edits to the active layout. Built-ins are never modified: edits
   * become a user copy named "<name> (custom)".
   */
  saveEdit(text: string): InstallResult {
    if (!this.active.builtin) {
      // The file name follows the layout name, so a rename installs a new file.
      const previous = this.active.id;
      const result = this.install(text, this.active.fileName);
      if (result.ok && result.entry.id !== previous) this.remove(previous);
      return result;
    }
    const result = parseLayout(text, this.active.layout.name);
    if (!result.ok) return result;
    const renamed =
      result.layout.name === this.active.layout.name ? withName(text, `${result.layout.name} (custom)`) : text;
    return this.install(renamed, this.active.fileName);
  }

  /** Installs the active layout, with this device's in-place edits applied, as a new user layout named `name`. */
  saveAs(name: string): InstallResult {
    const existing = this.user.find((e) => e.id === `user:${fileNameFor(name)}`);
    if (existing) {
      const message = `a layout named “${existing.layout.name}” already exists`;
      return { ok: false, errors: [{ message, line: null, column: null }] };
    }
    const layout = this.layout;
    const shown = (variant: Variant) => {
      const root = rootOf(layout, variant);
      return root && shownRoot(root, this.#overlayAt(editsKey(this.active.id, variant)));
    };
    const text = serializeLayout({ ...layout, name, root: shown('desktop')!, phone: shown('phone') });
    return this.install(text, fileNameFor(name));
  }

  remove(id: string) {
    this.user = this.user.filter((e) => e.id !== id);
    this.persist();
    this.#pruneEdits(id, null);
    for (const source of SOURCES) {
      if (this.activeBySource[source] === id) {
        this.activeBySource[source] = this.builtins[0].id;
        save(activeKey(source), null);
      }
    }
  }

  /** Sets or (with no size) clears the size of the node at `at`. */
  resize(at: At, size: Size | undefined) {
    this.#setShown(edit.resize(this.shown, at, size));
  }

  /** Moves the node at `from` beside the one at `to`. */
  move(from: At, to: At, side: Side) {
    this.#setShown(edit.move(this.shown, from, to, side));
  }

  /** Adds a widget beside the node at `at`. */
  addWidget(at: At, side: Side, widget: Widget) {
    this.#setShown(edit.insert(this.shown, at, side, edit.widget(widget)));
  }

  removeWidget(at: At) {
    this.#setShown(edit.remove(this.shown, at));
  }

  /** Drops the in-place edits to the tree on screen. */
  resetEdits() {
    this.#setShown(this.root);
  }

  #overlayAt(key: string): Overlay | undefined {
    const overlay = this.#edits[key];
    return overlay === null ? undefined : (overlay ?? loadEdits(key));
  }

  #setShown(shown: LayoutNode) {
    const of = fingerprint(this.root);
    this.#store(this.#editsKey, fingerprint(shown) === of ? null : { of, root: shown });
  }

  #store(key: string, overlay: Overlay | null) {
    this.#edits = { ...this.#edits, [key]: overlay };
    save(key, overlay && JSON.stringify(overlay));
  }

  /** Drops the edits that no longer fit a layout's trees, e.g. after its file changed. */
  #pruneEdits(id: string, layout: Layout | null) {
    for (const variant of VARIANTS) {
      const key = editsKey(id, variant);
      const root = layout && rootOf(layout, variant);
      const overlay = this.#overlayAt(key);
      if (overlay && (!root || overlay.of !== fingerprint(root))) this.#store(key, null);
    }
  }

  private persist() {
    const sources: Omit<LayoutSource, 'builtin'>[] = this.user.map(({ id, fileName, text }) => ({ id, fileName, text }));
    save(USER_KEY, JSON.stringify(sources));
  }
}

/** Sets `name=` on the `layout` node, preserving the file's comments and formatting. */
export function withName(text: string, name: string): string {
  const doc = parse(text);
  doc.findNodeByName('layout')?.setProperty('name', name);
  return format(doc);
}
