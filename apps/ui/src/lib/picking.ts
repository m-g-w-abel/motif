import type { IconName } from './components/Icon.svelte';
import type { TrackRef } from './protocol/TrackRef';

/**
 * What select mode acts on, for the phone's select bar (`SelectBar.svelte`),
 * set by the list that is selecting. Songs for now; an album or playlist
 * grid would hand over its picks' songs the same way.
 */
export interface Picking {
  count: number;
  /** Everything in the list is picked. */
  all: boolean;
  /** The picked songs, in list order. */
  tracks: () => TrackRef[];
  toggleAll: () => void;
  /** Delete: takes the picks out of the list (a playlist's entries). */
  remove?: () => void;
}

export type PickAction = { id: 'play' | 'queue' | 'playlist' | 'next' | 'all' | 'delete'; label: string; disabled: boolean };

/**
 * The select bar's actions: Play, Add to queue and Add to playlist as icons
 * on the bar, the rest in its ⋮ menu, each only where this device may run it
 * (`playlist`: `Playlists.canAdd` of the picks).
 */
export function pickActions(p: Picking, can: { playback: boolean; queue: boolean; playlist: boolean }) {
  const none = p.count === 0;
  const bar: (PickAction & { icon: IconName })[] = [];
  const more: PickAction[] = [];
  if (can.playback) bar.push({ id: 'play', label: 'Play', icon: 'play', disabled: none });
  if (can.queue) bar.push({ id: 'queue', label: 'Add to queue', icon: 'queue-add', disabled: none });
  if (can.playlist) bar.push({ id: 'playlist', label: 'Add to playlist…', icon: 'playlist-add', disabled: false });
  if (can.queue) more.push({ id: 'next', label: 'Queue next', disabled: none });
  more.push({ id: 'all', label: p.all ? 'Select none' : 'Select all', disabled: false });
  if (p.remove) more.push({ id: 'delete', label: 'Delete', disabled: none });
  return { bar, more };
}
