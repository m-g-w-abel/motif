import type { LibraryTrack } from './protocol/LibraryTrack';
import type { PairingState } from './protocol/PairingState';

/** Whether a track's audio is on linked engines, not this one: a row of their catalog (`remote:<key>:<id>`). */
export function elsewhere(track: LibraryTrack): boolean {
  return track.track.id.startsWith('remote:');
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
