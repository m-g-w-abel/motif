import { SvelteMap } from 'svelte/reactivity';
import type { EngineClient } from './engine.svelte';
import type { LibraryAlbum } from './protocol/LibraryAlbum';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { QueryResult } from './protocol/QueryResult';
import type { TrackGroup } from './protocol/TrackGroup';
import type { TrackRef } from './protocol/TrackRef';

/** Rows fetched per request. */
const PAGE = 200;

/**
 * A paged view of a library list (tracks or albums). Rows are fetched in
 * pages as they are asked for, and everything is refetched when the library
 * changes, so even a huge library never crosses the wire at once.
 */
abstract class PagedList<T> {
  total = $state(0);
  /** Set once the first page (and the total) has arrived. */
  loaded = $state(false);
  #pages = new Map<number, T[]>();
  #loading = new Set<number>();
  #version = $state(0);
  #revision = -1;

  constructor(
    protected engine: EngineClient,
    readonly search: string | null,
  ) {}

  /** Refetches if the library changed. Call from an `$effect`. */
  sync() {
    const revision = this.engine.library?.revision;
    if (revision === undefined || revision === this.#revision || this.engine.status !== 'open') return;
    this.#revision = revision;
    this.#pages.clear();
    this.#load(0);
    this.refresh();
  }

  /** Row `i`, or `undefined` while its page loads. */
  get(i: number): T | undefined {
    void this.#version;
    const page = Math.floor(i / PAGE);
    const rows = this.#pages.get(page);
    if (!rows) this.#load(page);
    return rows?.[i - page * PAGE];
  }

  #load(page: number) {
    if (this.#loading.has(page) || this.engine.status !== 'open') return;
    this.#loading.add(page);
    // Deferred: rows are asked for while rendering, which may not change state.
    queueMicrotask(() => {
      this.fetch(page * PAGE, PAGE)
        .then(({ total, rows }) => {
          this.#pages.set(page, rows);
          this.total = total;
          this.loaded = true;
          this.#version++;
        })
        .catch((e: Error) => console.warn('library page', e.message))
        .finally(() => this.#loading.delete(page));
    });
  }

  protected abstract fetch(offset: number, limit: number): Promise<{ total: number; rows: T[] }>;
  /** Fetches anything else that goes with the list. */
  protected refresh() {}
}

export class LibraryTracks extends PagedList<LibraryTrack> {
  /** Album runs in list order, for headers and for finding an album. */
  groups = $state.raw<TrackGroup[]>([]);

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
      queueMicrotask(() => this.#details.clear());
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
      .catch(() => paths.forEach((path) => this.#details.set(path, null)));
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
