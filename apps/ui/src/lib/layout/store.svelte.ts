import { format, parse } from '@bgotink/kdl';
import type { Layout } from './model';
import { parseLayout, type LayoutError } from './parse';
import { builtinLayouts, type LayoutSource } from './presets';

// Layouts are a client-side concern: user layouts and the choice of layout
// live in this device's storage, never in the engine.
const USER_KEY = 'layouts.user';
const SOURCE_KEY = 'browse.source';
const activeKey = (source: Source) => `layouts.active.${source}`;

/** What the music area is showing; picked in the left bar. Saved playlists join this in phase 1. */
export type Source = 'library';
export const SOURCES: Source[] = ['library'];

/**
 * Widget state that should survive switching layouts: the same selection and
 * album focus carry over whichever widgets are showing them.
 */
export interface BrowseState {
  selected: string | null;
  focusAlbum: string | null;
  /** What the library is filtered to; empty for everything. */
  search: string;
}

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
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
    const created: BrowseState = $state({ selected: null, focusAlbum: null, search: '' });
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

  remove(id: string) {
    this.user = this.user.filter((e) => e.id !== id);
    this.persist();
    for (const source of SOURCES) {
      if (this.activeBySource[source] === id) {
        this.activeBySource[source] = this.builtins[0].id;
        save(activeKey(source), null);
      }
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
