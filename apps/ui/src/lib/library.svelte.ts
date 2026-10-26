import { SvelteMap } from 'svelte/reactivity';
import type { EngineClient } from './engine.svelte';
import type { LibraryAlbum } from './protocol/LibraryAlbum';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { QueryResult } from './protocol/QueryResult';
import type { SortRule } from './protocol/SortRule';
import type { TrackGroup } from './protocol/TrackGroup';
import type { TrackRef } from './protocol/TrackRef';

/** Rows fetched per request. */
const PAGE = 200;
/** Fewest ms between refetches while the library keeps changing. */
const SYNC_INTERVAL = 1000;

/**
 * A paged view of a library list (tracks or albums). Rows are fetched in
 * pages as they are asked for, and everything is refetched when the library
 * changes, so even a huge library never crosses the wire at once.
 */
abstract class PagedList<T> {
  total = $state(0);
  /** Set once the first page (and the total) has arrived. */
  loaded = $state(false);
  /** Pages stay after a library change (stale) until their refetch lands. */
  #pages = new Map<number, { rows: T[]; revision: number }>();
  #loading = new Set<number>();
  #version = $state(0);
  #revision = -1;
  #synced = 0;
  #seeking = false;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #anchor: { index: number; key: string | null } | null = null;
  /** Called with the anchor row's new index when rows were inserted or removed above it. */
  onShift?: (index: number) => void;

  constructor(
    protected engine: EngineClient,
    readonly search: string | null,
    readonly sort: SortRule[] = [],
  ) {}

  /** Refetches if the library changed. Call from an `$effect`. */
  sync() {
    const revision = this.engine.library?.revision;
    if (revision === undefined || revision === this.#revision || this.engine.status !== 'open') return;
    // A scan bumps the revision every few hundred files; don't refetch on each.
    const wait = this.#synced + SYNC_INTERVAL - Date.now();
    if (this.#revision >= 0 && wait > 0) {
      this.#timer ??= setTimeout(() => {
        this.#timer = undefined;
        this.sync();
      }, wait);
      return;
    }
    this.#pin();
    this.#synced = Date.now();
    this.#revision = revision;
    this.#load(0);
    this.refresh();
  }

  /** Remembers the first visible row so a shift of the rows above it can be reported. */
  setAnchor(index: number) {
    this.#anchor = { index, key: null };
    this.#pin();
  }

  /** Names the anchor row; its page may not have been loaded when the anchor was set. */
  #pin() {
    const a = this.#anchor;
    const row = a && this.#pages.get(Math.floor(a.index / PAGE))?.rows[a.index % PAGE];
    if (a && row) a.key ??= this.key(row);
  }

  /** Row `i`, or `undefined` while its page loads. */
  get(i: number): T | undefined {
    void this.#version;
    const page = Math.floor(i / PAGE);
    const cached = this.#pages.get(page);
    if (cached?.revision !== this.#revision) this.#load(page);
    return cached?.rows[i - page * PAGE];
  }

  #load(page: number) {
    if (this.#loading.has(page) || this.engine.status !== 'open') return;
    this.#loading.add(page);
    // Deferred: rows are asked for while rendering, which may not change state.
    queueMicrotask(() => {
      const revision = this.#revision;
      this.fetch(page * PAGE, PAGE)
        .then(async ({ total, rows }) => {
          const a = this.#anchor;
          // The anchor row left its page: find it before swapping, so the view doesn't show the wrong rows meanwhile.
          if (a?.key && page === Math.floor(a.index / PAGE) && !this.#seeking && !rows.some((r) => this.key(r) === a.key)) {
            this.total = total;
            await this.#seek(a, revision);
          }
          this.#pages.set(page, { rows, revision });
          this.total = total;
          this.loaded = true;
          this.#version++;
          this.#land(page, rows);
        })
        .catch((e: Error) => console.warn('library page', e.message))
        .finally(() => this.#loading.delete(page));
    });
  }

  /** Moves the anchor if `rows` hold it at another index; true if they hold it. */
  #land(page: number, rows: T[]): boolean {
    const a = this.#anchor;
    const at = a?.key ? rows.findIndex((row) => this.key(row) === a.key) : -1;
    if (!a || at < 0) return false;
    if (page * PAGE + at !== a.index) {
      a.index = page * PAGE + at;
      this.onShift?.(a.index);
    }
    return true;
  }

  /** The anchor row left its page: look for it in nearby pages, nearest first. */
  async #seek(a: { index: number; key: string | null }, revision: number) {
    this.#seeking = true;
    const home = Math.floor(a.index / PAGE);
    const last = Math.ceil(this.total / PAGE) - 1;
    try {
      // shortcut: gives up after 10 pages each way; a bigger shift is left where it is.
      for (let k = 1; k <= 10 && this.#anchor === a && revision === this.#revision; k++) {
        for (const page of [home + k, home - k]) {
          if (page < 0 || page > last) continue;
          const { total, rows } = await this.fetch(page * PAGE, PAGE);
          this.#pages.set(page, { rows, revision });
          this.total = total;
          this.#version++;
          if (this.#land(page, rows)) return;
        }
      }
    } catch (e) {
      console.warn('library seek', (e as Error).message);
    } finally {
      this.#seeking = false;
    }
  }

  /** Identifies a row across refetches. */
  protected abstract key(row: T): string;
  protected abstract fetch(offset: number, limit: number): Promise<{ total: number; rows: T[] }>;
  /** Fetches anything else that goes with the list. */
  protected refresh() {}
}

export class LibraryTracks extends PagedList<LibraryTrack> {
  /** Album runs in list order, for headers and for finding an album. */
  groups = $state.raw<TrackGroup[]>([]);

  protected key(row: LibraryTrack) {
    return row.track.id;
  }

  protected async fetch(offset: number, limit: number) {
    const result = await this.engine.query({ type: 'tracks', search: this.search, sort: this.sort, offset, limit });
    if (result.type !== 'tracks') throw new Error(`unexpected ${result.type}`);
    return { total: result.total, rows: result.tracks };
  }

  protected refresh() {
    this.engine.query({ type: 'groups', search: this.search, sort: this.sort }).then((result: QueryResult) => {
      if (result.type === 'groups') this.groups = result.groups;
    });
  }
}

export class LibraryAlbums extends PagedList<LibraryAlbum> {
  protected key(row: LibraryAlbum) {
    return row.key;
  }

  protected async fetch(offset: number, limit: number) {
    const result = await this.engine.query({ type: 'albums', search: this.search, sort: this.sort, offset, limit });
    if (result.type !== 'albums') throw new Error(`unexpected ${result.type}`);
    return { total: result.total, rows: result.albums };
  }
}

/**
 * Shared library state for the UI: list views (cached per search and order, so
 * switching layouts reuses fetched pages) and details looked up per track.
 */
export class LibraryClient {
  #tracks = new Map<string, LibraryTracks>();
  #albums = new Map<string, LibraryAlbums>();
  /** Path → library details, or `null` if the file isn't in the library. */
  #details = new SvelteMap<string, LibraryTrack | null>();
  #wanted = new Set<string>();
  #revision = -1;
  #refreshed = 0;
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  get status() {
    return this.engine.library;
  }

  /** With a `shown` slot of its own, a caller keeps the last list until the new one's first page is in. */
  tracks(search: string | null, sort: SortRule[] = [], shown?: Shown<LibraryTracks>): LibraryTracks {
    const list = cached(this.#tracks, search, sort, () => new LibraryTracks(this.engine, search, sort));
    return shown ? hold(list, shown) : list;
  }

  albums(search: string | null, sort: SortRule[] = [], shown?: Shown<LibraryAlbums>): LibraryAlbums {
    const list = cached(this.#albums, search, sort, () => new LibraryAlbums(this.engine, search, sort));
    return shown ? hold(list, shown) : list;
  }

  /** Library details for a queued or playing track; `undefined` while being looked up. */
  details(track: TrackRef | undefined): LibraryTrack | null | undefined {
    if (track?.source !== 'local') return null;
    // Details can change with the library (e.g. new artwork).
    const revision = this.engine.library?.revision ?? -1;
    if (revision !== this.#revision && Date.now() - this.#refreshed >= SYNC_INTERVAL) {
      this.#revision = revision;
      this.#refreshed = Date.now();
      // Keep what's known (stale) and look it all up again.
      for (const path of this.#details.keys()) this.#wanted.add(path);
      if (this.#wanted.size) queueMicrotask(() => this.#lookup());
    }
    const known = this.#details.get(track.id);
    if (known === undefined && !this.#wanted.has(track.id)) {
      this.#wanted.add(track.id);
      // Batch everything asked for in one render into one lookup.
      if (this.#wanted.size === 1) queueMicrotask(() => this.#lookup());
    }
    return known;
  }

  #lookup() {
    const paths = [...this.#wanted];
    this.#wanted.clear();
    if (!paths.length) return;
    if (this.engine.status !== 'open' || !this.engine.library) {
      for (const path of paths) this.#details.set(path, null);
      return;
    }
    this.engine
      .query({ type: 'lookup', paths })
      .then((result) => {
        const found = new Map(result.type === 'tracks' ? result.tracks.map((t) => [t.track.id, t]) : []);
        for (const path of paths) this.#details.set(path, found.get(path) ?? null);
      })
      .catch(() => paths.forEach((path) => this.#details.get(path) === undefined && this.#details.set(path, null)));
  }

  /** URL of artwork `art`, scaled to fit `size` pixels. */
  artUrl(art: string, size: number): string {
    // Whole steps keep the server's thumbnail cache small.
    const step = size <= 64 ? 64 : size <= 128 ? 128 : size <= 256 ? 256 : 512;
    return `${this.engine.httpBase}/art/${art}?size=${Math.round(step * (globalThis.devicePixelRatio ?? 1))}`;
  }
}

function cached<T>(map: Map<string, T>, search: string | null, sort: SortRule[], make: () => T): T {
  const key = JSON.stringify([search ?? '', sort]);
  let value = map.get(key);
  if (!value) {
    // Keep a handful of searches and orders; the oldest go first.
    if (map.size > 8) map.delete(map.keys().next().value!);
    value = make();
    map.set(key, value);
  }
  return value;
}

/** What a caller last showed. */
export interface Shown<T> {
  list?: T;
}

/** `next` once its first page is in; until then what was shown stays, so a new order doesn't blank the view. */
function hold<T extends PagedList<unknown>>(next: T, shown: Shown<T>): T {
  if (!next.loaded && shown.list) {
    next.sync(); // start the fetch; `loaded` flipping re-runs whoever asked
    return shown.list;
  }
  return (shown.list = next);
}
