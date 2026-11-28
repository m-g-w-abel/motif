import { contextMenu, type MenuItem } from './contextMenu.svelte';
import type { EngineClient } from './engine.svelte';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { Playlist } from './protocol/Playlist';
import type { PlaylistEntry } from './protocol/PlaylistEntry';
import type { TrackRef } from './protocol/TrackRef';
import { moveItem } from './reorder';
import { toasts } from './toast.svelte';

/** An id for a playlist or an addition, made here so a retried command finds its earlier self. */
// crypto.randomUUID needs a secure context, which a phone browsing an engine over http isn't.
const newId = () => Array.from(crypto.getRandomValues(new Uint8Array(16)), (b) => b.toString(16).padStart(2, '0')).join('');

/** A drag of entry `from` to index `to`, as the engine takes it: the entry and the one it now goes before. */
export function moveCommand(items: readonly string[], from: number, to: number): { item: string; before: string | null } {
  const next = moveItem(items, from, to);
  return { item: items[from], before: next[to + 1] ?? null };
}

/** Saved playlists, kept by the engine's library. */
export class Playlists {
  list = $state<Playlist[]>([]);
  /** What `PlaylistPrompt.svelte` is asking: a name (`value`), or to confirm `action` when `value` is null (`detail` saying why). */
  asking = $state<{ title: string; detail?: string; value: string | null; action: string; done: (answer: string | null) => void } | null>(null);
  #synced = '';
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  /** Changes whenever a playlist or the library does, for pages to refetch on. */
  get revision(): string {
    const status = this.engine.library;
    return status && this.engine.status === 'open' ? `${status.playlists}/${status.revision}` : '';
  }

  /** Refetches the list when a playlist or the library changes; call from an `$effect`. */
  sync() {
    const key = this.revision;
    if (!key || key === this.#synced) return;
    this.#synced = key;
    this.engine
      .query({ type: 'playlists' })
      .then((r) => r.type === 'playlists' && (this.list = r.playlists))
      .catch(() => {});
  }

  /** A playlist's entries in order. */
  async entries(id: string): Promise<PlaylistEntry[]> {
    const r = await this.engine.query({ type: 'playlist_tracks', id });
    return r.type === 'playlist_tracks' ? r.entries : [];
  }

  #ask(title: string, value: string | null, action: string, detail?: string): Promise<string | null> {
    this.asking?.done(null);
    return new Promise((done) => (this.asking = { title, detail, value, action, done }));
  }

  /** Asks to confirm `action` (`detail` saying why); the same dialog as a playlist's Delete. */
  async confirm(title: string, detail: string, action: string): Promise<boolean> {
    return (await this.#ask(title, null, action, detail)) !== null;
  }

  /** Asks for a name; `null` if cancelled or left empty. */
  askName(title: string, value = ''): Promise<string | null> {
    return this.#ask(title, value, 'Save').then((name) => name?.trim() || null);
  }

  /** The prompt's answer: the name, `''` to confirm, `null` to cancel. */
  answer(answer: string | null) {
    const asking = this.asking;
    this.asking = null;
    asking?.done(answer);
  }

  #run(query: LibraryQuery): Promise<boolean> {
    return this.engine.query(query).then(
      () => true,
      (e: Error) => {
        this.engine.error = e.message;
        return false;
      },
    );
  }

  /** Whether it was made (not if the name was cancelled). */
  async create(tracks: TrackRef[] | Promise<TrackRef[]> = []): Promise<boolean> {
    const name = await this.askName('New playlist');
    if (!name) return false;
    const paths = local(await tracks);
    const made = await this.#run({ type: 'create_playlist', id: newId(), name, paths });
    if (made) toasts.show(paths.length ? `Added to ${name}` : `Made ${name}`);
    return made;
  }

  async add(playlist: Playlist, tracks: TrackRef[] | Promise<TrackRef[]>): Promise<boolean> {
    const added = await this.#run({ type: 'add_to_playlist', id: playlist.id, op: newId(), paths: local(await tracks) });
    if (added) toasts.show(`Added to ${playlist.name}`);
    return added;
  }

  async rename(playlist: Playlist) {
    const name = await this.askName('Rename playlist', playlist.name);
    if (name) void this.#run({ type: 'rename_playlist', id: playlist.id, name });
  }

  /** Deletes after asking; `onDelete` runs once it is decided (e.g. leaves the playlist's page). */
  async delete(playlist: Playlist, onDelete?: () => void) {
    if ((await this.#ask(`Delete ${playlist.name}?`, null, 'Delete')) === null) return;
    onDelete?.();
    void this.#run({ type: 'delete_playlist', id: playlist.id });
  }

  play(id: string, start = 0) {
    void this.#run({ type: 'play_playlist', id, start });
  }

  remove(id: string, items: string[]) {
    void this.#run({ type: 'remove_from_playlist', id, items });
  }

  /** Moves entry `from` of `items` (the playlist's entries, in order) to `to`. */
  move(id: string, items: string[], from: number, to: number) {
    void this.#run({ type: 'move_playlist_item', id, ...moveCommand(items, from, to) });
  }

  /** Whether this device may add `tracks` to a playlist: it may edit them and some track is a library file. */
  canAdd(tracks: TrackRef[]): boolean {
    return this.engine.can('library') && local(tracks).length > 0;
  }

  /** Where `tracks` (or those still being fetched) can go: a new playlist, then each one; `then` runs once they are in one. */
  choices(tracks: TrackRef[] | Promise<TrackRef[]>, then?: () => void): MenuItem[] {
    const after = (added: Promise<boolean>) => void added.then((ok) => ok && then?.());
    return [{ label: 'New playlist…', run: () => after(this.create(tracks)) }, ...this.list.map((p) => ({ label: p.name, run: () => after(this.add(p, tracks)) }))];
  }

  /** Opens the menu of playlists to add `tracks` to, at `x`, `y`. */
  pick(tracks: TrackRef[], x: number, y: number, origin: HTMLElement | null) {
    contextMenu.show(this.choices(tracks), x, y, origin);
  }

  /** "Add to playlist…", opening a menu of playlists; nothing unless {@link canAdd}. */
  addItems(tracks: TrackRef[]): MenuItem[] {
    if (!this.canAdd(tracks)) return [];
    const choose = () => this.pick(tracks, contextMenu.x, contextMenu.y, document.activeElement as HTMLElement | null);
    return [{ label: 'Add to playlist…', run: choose }];
  }

  /** The ⋮ menu of a playlist; `onDelete` as for `delete`. */
  menu(playlist: Playlist, onDelete?: () => void): MenuItem[] {
    const items: MenuItem[] = [];
    if (this.engine.can('playback') && playlist.tracks) items.push({ label: 'Play', run: () => this.play(playlist.id) });
    if (this.engine.can('library')) {
      items.push({ label: 'Rename…', run: () => this.rename(playlist) }, { label: 'Delete', run: () => this.delete(playlist, onDelete) });
    }
    return items;
  }
}

/** Playlists hold library files only. */
const local = (tracks: TrackRef[]) => tracks.filter((t) => t.source === 'local').map((t) => t.id);
