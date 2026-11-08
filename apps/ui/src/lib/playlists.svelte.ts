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

/** Saved playlists, kept by the engine's library, and the one whose page shows in the music area. */
export class Playlists {
  list = $state<Playlist[]>([]);
  /** The playlist page showing instead of the layout, if any. */
  shown = $state<string | null>(null);
  /** The shown playlist's entries. */
  entries = $state<PlaylistEntry[]>([]);
  /** What `PlaylistPrompt.svelte` is asking: a name (`value`), or to confirm `action` when `value` is null. */
  asking = $state<{ title: string; value: string | null; action: string; done: (answer: string | null) => void } | null>(null);
  #synced = '';
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  get current(): Playlist | undefined {
    return this.list.find((p) => p.id === this.shown);
  }

  /** Refetches when a playlist or the library changes; call from an `$effect`. */
  sync() {
    const status = this.engine.library;
    if (!status || this.engine.status !== 'open') return;
    const key = `${status.playlists}/${status.revision}/${this.shown}`;
    if (key === this.#synced) return;
    this.#synced = key;
    const shown = this.shown;
    this.engine
      .query({ type: 'playlists' })
      .then((r) => r.type === 'playlists' && (this.list = r.playlists))
      .catch(() => {});
    if (!shown) return;
    this.engine
      .query({ type: 'playlist_tracks', id: shown })
      .then((r) => r.type === 'playlist_tracks' && this.shown === shown && (this.entries = r.entries))
      .catch(() => {});
  }

  show(id: string | null) {
    if (id !== this.shown) this.entries = [];
    this.shown = id;
  }

  #ask(title: string, value: string | null, action: string): Promise<string | null> {
    this.asking?.done(null);
    return new Promise((done) => (this.asking = { title, value, action, done }));
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

  async create(tracks: TrackRef[] = []) {
    const name = await this.askName('New playlist');
    if (!name) return;
    const paths = local(tracks);
    if (await this.#run({ type: 'create_playlist', id: newId(), name, paths })) {
      toasts.show(paths.length ? `Added to ${name}` : `Made ${name}`);
    }
  }

  async add(playlist: Playlist, tracks: TrackRef[]) {
    if (await this.#run({ type: 'add_to_playlist', id: playlist.id, op: newId(), paths: local(tracks) })) {
      toasts.show(`Added to ${playlist.name}`);
    }
  }

  async rename(playlist: Playlist) {
    const name = await this.askName('Rename playlist', playlist.name);
    if (name) void this.#run({ type: 'rename_playlist', id: playlist.id, name });
  }

  async delete(playlist: Playlist) {
    if ((await this.#ask(`Delete ${playlist.name}?`, null, 'Delete')) === null) return;
    if (this.shown === playlist.id) this.show(null);
    void this.#run({ type: 'delete_playlist', id: playlist.id });
  }

  play(id: string, start = 0) {
    void this.#run({ type: 'play_playlist', id, start });
  }

  remove(id: string, items: string[]) {
    this.entries = this.entries.filter((e) => !items.includes(e.item));
    void this.#run({ type: 'remove_from_playlist', id, items });
  }

  /** Moves entry `from` to `to` on the shown page at once, then tells the engine. */
  move(id: string, from: number, to: number) {
    const { item, before } = moveCommand(
      this.entries.map((e) => e.item),
      from,
      to,
    );
    this.entries = moveItem(this.entries, from, to);
    void this.#run({ type: 'move_playlist_item', id, item, before });
  }

  /** "Add to playlist…", opening a menu of playlists; nothing if this device may not edit them or no track is a library file. */
  addItems(tracks: TrackRef[]): MenuItem[] {
    if (!this.engine.can('library') || !local(tracks).length) return [];
    const choose = () =>
      contextMenu.show(
        [{ label: 'New playlist…', run: () => this.create(tracks) }, ...this.list.map((p) => ({ label: p.name, run: () => this.add(p, tracks) }))],
        contextMenu.x,
        contextMenu.y,
        document.activeElement as HTMLElement | null,
      );
    return [{ label: 'Add to playlist…', run: choose }];
  }

  /** The ⋮ menu of a playlist. */
  menu(playlist: Playlist): MenuItem[] {
    const items: MenuItem[] = [];
    if (this.engine.can('playback') && playlist.tracks) items.push({ label: 'Play', run: () => this.play(playlist.id) });
    if (this.engine.can('library')) {
      items.push({ label: 'Rename…', run: () => this.rename(playlist) }, { label: 'Delete', run: () => this.delete(playlist) });
    }
    return items;
  }
}

/** Playlists hold library files only. */
const local = (tracks: TrackRef[]) => tracks.filter((t) => t.source === 'local').map((t) => t.id);
