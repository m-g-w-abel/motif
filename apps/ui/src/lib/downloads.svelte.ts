import type { MenuItem } from './contextMenu.svelte';
import { elsewhere } from './devices';
import type { EngineClient } from './engine.svelte';
import { NO_FILTER } from './library.svelte';
import type { Downloadable } from './protocol/Downloadable';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { TrackRef } from './protocol/TrackRef';
import { toasts } from './toast.svelte';

/** A member's identity, for comparing them. */
export const keyOf = (m: Downloadable) =>
  m.kind === 'playlist' ? `playlist:${m.id}` : m.kind === 'album' ? `album:${m.album}` : m.kind === 'artist' ? `artist:${m.name}` : `track:${m.path}`;

/** How the Downloaded view names a member that isn't a playlist or a track (those are looked up). */
export function memberLabel(m: Downloadable): { title: string; line: string } {
  if (m.kind === 'album') {
    // The library keys albums as album artist, unit separator, title (`albumKey`).
    const [artist, title] = m.album.split('\u001f');
    return { title: title || 'Unknown album', line: artist ? `Album · ${artist}` : 'Album' };
  }
  if (m.kind === 'artist') return { title: m.name || 'Unknown artist', line: 'Artist' };
  if (m.kind === 'playlist') return { title: 'Playlist', line: 'Playlist' };
  return { title: m.path.split('/').at(-1) ?? m.path, line: 'Song' };
}

/** How many of a member's songs are on this device: a downloaded copy shadows the linked engine's row. */
export function progress(rows: { track: TrackRef }[]): { here: number; total: number } {
  return { here: rows.filter((r) => !elsewhere(r)).length, total: rows.length };
}

/** Where a member stands: not in the group, downloading (`share` of its songs here, 0 to 1), or all here. */
export type MemberState = { kind: 'off' } | { kind: 'downloading'; share: number } | { kind: 'done' };

/**
 * The downloaded copies among `rows` that no linked engine holds any more (`linked`: their keys), which
 * taking them out deletes for good.
 */
export function onlyCopies(rows: LibraryTrack[], isCopy: (t: TrackRef) => boolean, linked: Set<string>): LibraryTrack[] {
  return rows.filter((r) => isCopy(r.track) && !r.devices.some((d) => linked.has(d)));
}

/** Library tracks as members of their own. */
export const trackMembers = (tracks: TrackRef[]): Downloadable[] =>
  tracks.filter((t) => t.source === 'local').map((t) => ({ kind: 'track', path: t.id }));

/** This device's Downloaded group (#245), kept by its engine: what it keeps copies of from linked engines. */
export class Downloads {
  members = $state.raw<Downloadable[]>([]);
  #keys = $derived(new Set(this.members.map(keyOf)));
  /** Each member's songs here and in all, by member; worked out again as the library changes. */
  progress = $state.raw(new Map<string, { here: number; total: number }>());
  #synced = '';
  private engine: EngineClient;
  /** Asks the user to confirm `action`, `detail` saying why; true to go ahead. */
  private confirm: (title: string, detail: string, action: string) => Promise<boolean>;

  constructor(engine: EngineClient, confirm: (title: string, detail: string, action: string) => Promise<boolean> = async () => true) {
    this.engine = engine;
    this.confirm = confirm;
  }

  /** How downloading is going: the folder, what's left and why the last one failed. */
  get status() {
    return this.engine.library?.downloads ?? null;
  }

  /** Refetches the members when the library changes; call from an `$effect`. */
  sync() {
    const status = this.engine.library;
    const key = status && this.engine.status === 'open' ? `${status.revision}/${status.playlists}` : '';
    if (!key || key === this.#synced) return;
    this.#synced = key;
    this.engine
      .query({ type: 'downloaded' })
      .then(async (r) => {
        if (r.type !== 'downloaded') return;
        this.members = r.members;
        const counts = await Promise.all(r.members.map((m) => this.tracksOf(m).then(progress, () => undefined)));
        if (this.#synced !== key) return;
        this.progress = new Map(r.members.flatMap((m, i) => (counts[i] ? [[keyOf(m), counts[i]]] : [])));
      })
      .catch(() => {});
  }

  /** How far `member` is: off, downloading or done (done too while its songs are still being counted). */
  state(member: Downloadable): MemberState {
    if (!this.has(member)) return { kind: 'off' };
    const p = this.progress.get(keyOf(member));
    return p && p.here < p.total ? { kind: 'downloading', share: p.here / p.total } : { kind: 'done' };
  }

  async #songs(albums: string[]): Promise<LibraryTrack[]> {
    if (!albums.length) return [];
    const r = await this.engine.query({ type: 'album_tracks', albums, sort: [], filter: NO_FILTER });
    return r.type === 'tracks' ? r.tracks : [];
  }

  /**
   * A member's songs, each as the library plays it: a downloaded copy where there is one. A playlist's entries
   * and a looked-up song are the linked engine's rows even once a copy shadows them; their albums list the one that wins.
   */
  async tracksOf(m: Downloadable): Promise<LibraryTrack[]> {
    if (m.kind === 'album') return this.#songs([m.album]);
    if (m.kind === 'artist') {
      // shortcut: an artist's first 500 albums; page them if a collector has more.
      const r = await this.engine.query({ type: 'albums', search: null, artist: m.name, sort: [], filter: NO_FILTER, offset: 0, limit: 500 });
      return this.#songs(r.type === 'albums' ? r.albums.map((a) => a.key) : []);
    }
    const r = await this.engine.query(m.kind === 'playlist' ? { type: 'playlist_tracks', id: m.id } : { type: 'lookup', paths: [m.path] });
    const tracks = r.type === 'playlist_tracks' ? r.entries.map((e) => e.track) : r.type === 'tracks' ? r.tracks : [];
    const albumOf = (t: LibraryTrack) => `${t.album_artist ?? t.track.artist ?? ''}\u001f${t.track.album ?? ''}`;
    const winners = await this.#songs([...new Set(tracks.map(albumOf))]);
    const same = (a: LibraryTrack, b: LibraryTrack) => albumOf(a) === albumOf(b) && a.track.title === b.track.title && a.track_no === b.track_no;
    return tracks.map((t) => winners.find((w) => same(w, t)) ?? t);
  }

  /** Takes `members` out, first asking when that deletes the only copy of some song (no linked engine has it). */
  async remove(members: Downloadable[]) {
    const linked = new Set((this.engine.pairing?.devices ?? []).flatMap((d) => (d.public_key ? [d.public_key] : [])));
    const rows = (await Promise.all(members.map((m) => this.tracksOf(m).catch(() => [])))).flat();
    const only = onlyCopies(rows, (t) => this.isCopy(t), linked).length;
    if (only) {
      const songs = only === 1 ? 'One song is' : `${only} songs are`;
      const go = await this.confirm('Remove download?', `${songs} only on this device: no linked device has them any more. Removing the download deletes them.`, 'Remove');
      if (!go) return;
    }
    this.set(members, false);
    toasts.show('Removed from Downloaded');
  }

  has(member: Downloadable): boolean {
    return this.#keys.has(keyOf(member));
  }

  /** Adds or takes out each of `members`; shows at once, the engine following. */
  set(members: Downloadable[], downloaded: boolean) {
    const keys = new Set(members.map(keyOf));
    this.members = [...this.members.filter((m) => !keys.has(keyOf(m))), ...(downloaded ? members : [])];
    for (const member of members) {
      this.engine.query({ type: 'set_downloaded', member, downloaded }).catch((e: Error) => (this.engine.error = e.message));
    }
  }

  /** A library file that is a downloaded copy: it sits in the download folder. */
  isCopy(track: Pick<TrackRef, 'source' | 'id'>): boolean {
    const folder = this.status?.folder.replace(/[\\/]+$/, '');
    return !!folder && track.source === 'local' && track.id.startsWith(folder) && /^[\\/]/.test(track.id.slice(folder.length));
  }

  /**
   * Download, or Remove download once all of `members` are in. Only where this device may change the library,
   * and, as downloads come from linked engines, only with one linked (or for what is in already).
   */
  items(members: Downloadable[]): MenuItem[] {
    if (!members.length || !this.engine.can('library')) return [];
    const all = members.every((m) => this.has(m));
    const linked = this.engine.pairing ? this.engine.pairing.net.links.length > 0 : true;
    if (!all && !linked && !members.some((m) => this.has(m))) return [];
    return [
      all
        ? { label: 'Remove download', run: () => void this.remove(members) }
        : { label: 'Download', run: () => (this.set(members, true), toasts.show('Added to Downloaded')) },
    ];
  }
}
