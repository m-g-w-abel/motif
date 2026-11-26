import type { MenuItem } from './contextMenu.svelte';
import { elsewhere } from './devices';
import type { EngineClient } from './engine.svelte';
import type { Downloadable } from './protocol/Downloadable';
import type { TrackRef } from './protocol/TrackRef';
import { toasts } from './toast.svelte';

/** A member's identity, for comparing them. */
const keyOf = (m: Downloadable) =>
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

/** Library tracks as members of their own. */
export const trackMembers = (tracks: TrackRef[]): Downloadable[] =>
  tracks.filter((t) => t.source === 'local').map((t) => ({ kind: 'track', path: t.id }));

/** This device's Downloaded group (#245), kept by its engine: what it keeps copies of from linked engines. */
export class Downloads {
  members = $state.raw<Downloadable[]>([]);
  #keys = $derived(new Set(this.members.map(keyOf)));
  #synced = '';
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
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
      .then((r) => r.type === 'downloaded' && (this.members = r.members))
      .catch(() => {});
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
        ? { label: 'Remove download', run: () => (this.set(members, false), toasts.show('Removed from Downloaded')) }
        : { label: 'Download', run: () => (this.set(members, true), toasts.show('Added to Downloaded')) },
    ];
  }
}
