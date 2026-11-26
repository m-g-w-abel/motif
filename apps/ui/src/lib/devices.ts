import type { SavedEngine } from './engines';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { LinkState } from './protocol/LinkState';
import type { PairedDevice } from './protocol/PairedDevice';
import type { PairingState } from './protocol/PairingState';
import type { Reach } from './protocol/Reach';
import type { TrackRef } from './protocol/TrackRef';

/** Whether a track's audio is on linked engines, not this one: a row of their catalog (`remote:<key>:<id>`). */
export function elsewhere({ track }: { track: TrackRef }): boolean {
  return track.id.startsWith('remote:');
}

/**
 * The devices holding a track that is elsewhere (“Laptop or Desk”) while none of them can be reached, so it
 * can't play now; null if one can, the track is here, or reach is unknown (only Full clients get `pairing`).
 */
export function cantReach(track: LibraryTrack | undefined, pairing: PairingState | null): string | null {
  if (!track || !pairing || !elsewhere(track)) return null;
  const reached = holders(track, pairing).some((d) => d && (pairing.net.links.find((l) => l.device === d.id)?.reach ?? 'unreachable') !== 'unreachable');
  return reached ? null : heldOn(track, pairing);
}

/** The paired devices holding a track, by name (“Laptop or Desk”). */
export function heldOn(track: LibraryTrack, pairing: PairingState | null): string {
  const names = holders(track, pairing).map((d) => d?.name).filter((n) => n !== undefined);
  return names.length ? names.join(' or ') : 'another device';
}

const holders = (track: LibraryTrack, pairing: PairingState | null) =>
  track.devices.map((key) => pairing?.devices.find((d) => d.public_key === key));

/** A row's device mark: what its tooltip says, and whether the row dims; null for a song here. */
export function deviceMark(track: LibraryTrack | undefined, pairing: PairingState | null): { title: string; away: boolean } | null {
  if (!track || !elsewhere(track)) return null;
  const away = cantReach(track, pairing);
  return away ? { title: `On ${away}, which can’t be reached now`, away: true } : { title: `On ${heldOn(track, pairing)}`, away: false };
}

/** An album's or playlist's page mark when any of its songs is elsewhere; it dims while none of those can be reached. */
export function pageMark(tracks: (LibraryTrack | undefined)[], pairing: PairingState | null): { title: string; away: boolean } | null {
  const away = tracks.filter((t): t is LibraryTrack => t !== undefined && elsewhere(t));
  if (!away.length) return null;
  const names = [...new Set(away.flatMap((t) => holders(t, pairing).map((d) => d?.name ?? 'another device')))].join(' or ');
  return away.every((t) => cantReach(t, pairing))
    ? { title: `Some songs are on ${names}, which can’t be reached now`, away: true }
    : { title: `Some songs are on ${names}`, away: false };
}

/** One other device in Settings → Devices: what this app controls and what controls the engine in control, joined by engine key. */
export interface DeviceRow {
  /** The engine key; the paired device's id for a client with none (a browser). */
  key: string;
  name: string;
  browser: boolean;
  /** Paired with the engine in control, so it can control that engine. */
  device: PairedDevice | null;
  /** In this app's switcher, so this app can control it. */
  saved: SavedEngine | null;
  linked: boolean;
  /** How it is reached now, from the link to it; null where there is none (not linked, or an engine only this app knows). */
  reach: Reach | null;
}

/**
 * Every other device once. `engines` are this app's (without the one in control, whose list `devices` is),
 * `devices` and `links` the engine in control's. Shortcut: this app's own engine, when it isn't the one in
 * control, shows under its own entry and again as a paired device if that engine paired it; there's no key to match.
 */
export function deviceRows(engines: SavedEngine[], devices: PairedDevice[], links: LinkState[]): DeviceRow[] {
  const rows = new Map<string, DeviceRow>();
  for (const saved of engines) rows.set(saved.id, { key: saved.id, name: saved.name, browser: false, device: null, saved, linked: false, reach: null });
  for (const device of devices) {
    const key = device.public_key ?? device.id;
    const row = rows.get(key) ?? { key, name: device.name, browser: !device.public_key, device: null, saved: null, linked: false, reach: null };
    row.device = device;
    row.linked = !!device.public_key && device.scopes.includes('sync');
    row.reach = links.find((l) => l.device === device.id)?.reach ?? null;
    rows.set(key, row);
  }
  return [...rows.values()];
}
