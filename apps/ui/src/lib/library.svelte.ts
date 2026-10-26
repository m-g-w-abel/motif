import { SvelteMap } from 'svelte/reactivity';
import type { EngineClient } from './engine.svelte';
import type { LibraryAlbum } from './protocol/LibraryAlbum';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { QueryResult } from './protocol/QueryResult';
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
  #timer: ReturnType<typeof setTimeout> | undefined;
  #anchor: { index: number; key: string } | null = null;
  /** Called with the anchor row's new index when rows were inserted or removed above it. */
  onShift?: (index: number) => void;

  constructor(
    protected engine: EngineClient,
    readonly search: string | null,
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
    this.#synced = Date.now();
    this.#revision = revision;
    this.#load(0);
    this.refresh();
  }

  /** Remembers the first visible row so a shift of the rows above it can be reported. */
  setAnchor(index: number) {
    const row = this.#pages.get(Math.floor(index / PAGE))?.rows[index % PAGE];
    this.#anchor = row ? { index, key: this.key(row) } : null;
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
        .then(({ total, rows }) => {
          this.#pages.set(page, { rows, revision });
          this.total = total;
          this.loaded = true;
          this.#version++;
          const a = this.#anchor;
          const at = a ? rows.findIndex((row) => this.key(row) === a.key) : -1;
          if (a && at >= 0 && page * PAGE + at !== a.index) {
            a.index = page * PAGE + at;
            this.onShift?.(a.index);
          }
        })
        .catch((e: Error) => console.warn('library page', e.message))
        .finally(() => this.#loading.delete(page));
    });
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
    const result = await this.engine.query({ type: 'tracks', search: this.search, offset, limit });
    if (result.type !== 'tracks') throw new Error(`unexpected ${result.type}`);
    return { total: result.total, rows: result.tracks };
  }

  protected refresh() {
    this.engine.query({ type: 'groups', search: this.search }).then((result: QueryResult) => {
      if (result.type === 'groups') this.groups = result.groups;
    });
  }
}

export class LibraryAlbums extends PagedList<LibraryAlbum> {
  protected key(row: LibraryAlbum) {
    return row.key;
  }

  protected async fetch(offset: number, limit: number) {
    const result = await this.engine.query({ type: 'albums', search: this.search, offset, limit });
    if (result.type !== 'albums') throw new Error(`unexpected ${result.type}`);
    return { total: result.total, rows: result.albums };
  }
}

/**
 * Shared library state for the UI: list views (cached per search, so
 * switching layouts reuses fetched pages) and details looked up per track.
 */
export class LibraryClient {
  #tracks = new Map<string, LibraryTracks>();
  #albums = new Map<string, LibraryAlbums>();
  /** Path → library details, or `null` if the file isn't in the library. */
  #details = new SvelteMap<string, LibraryTrack | null>();
  #wanted = new Set<string>();
  #revision = -1;
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  get status() {
    return this.engine.library;
  }

  tracks(search: string | null): LibraryTracks {
    return cached(this.#tracks, search, () => new LibraryTracks(this.engine, search));
  }

  albums(search: string | null): LibraryAlbums {
    return cached(this.#albums, search, () => new LibraryAlbums(this.engine, search));
  }

  /** Library details for a queued or playing track; `undefined` while being looked up. */
  details(track: TrackRef | undefined): LibraryTrack | null | undefined {
    if (track?.source !== 'local') return null;
    // Details can change with the library (e.g. new artwork).
    const revision = this.engine.library?.revision ?? -1;
    if (revision !== this.#revision) {
      this.#revision = revision;
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

function cached<T>(map: Map<string, T>, search: string | null, make: () => T): T {
  const key = search ?? '';
  let value = map.get(key);
  if (!value) {
    // Keep a handful of searches; the oldest go first.
    if (map.size > 8) map.delete(map.keys().next().value!);
    value = make();
    map.set(key, value);
  }
  return value;
}
