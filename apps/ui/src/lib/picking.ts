import type { IconName } from './components/Icon.svelte';
import type { EngineClient } from './engine.svelte';
import { playTracks, queueTracks } from './menus';
import type { Playlists } from './playlists.svelte';
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

export type PickAction = { id: 'play' | 'queue' | 'playlist' | 'next' | 'all' | 'delete'; label: string };

/**
 * The select bar's actions: Play, Add to queue and Add to playlist as icons
 * on the bar, the rest in its ⋮ menu, each only where this device may run it.
 * All of them show from the start, picks or not (`runPick`).
 */
export function pickActions(p: Picking, can: { playback: boolean; queue: boolean; playlist: boolean }) {
  const bar: (PickAction & { icon: IconName })[] = [];
  const more: PickAction[] = [];
  if (can.playback) bar.push({ id: 'play', label: 'Play', icon: 'play' });
  if (can.queue) bar.push({ id: 'queue', label: 'Add to queue', icon: 'queue-add' });
  if (can.playlist) bar.push({ id: 'playlist', label: 'Add to playlist…', icon: 'playlist-add' });
  if (can.queue) more.push({ id: 'next', label: 'Queue next' });
  more.push({ id: 'all', label: p.all ? 'Select none' : 'Select all' });
  if (p.remove) more.push({ id: 'delete', label: 'Delete' });
  return { bar, more };
}

/** Runs one of `pickActions` on the picks; with none picked, only Select all does anything. */
export function runPick(id: PickAction['id'], p: Picking, engine: EngineClient, playlists: Playlists, anchor: HTMLElement) {
  if (id === 'all') return p.toggleAll();
  const tracks = p.tracks();
  if (!p.count) return;
  if (id === 'play') playTracks(engine, tracks);
  else if (id === 'queue') void queueTracks(engine, tracks);
  else if (id === 'next') void queueTracks(engine, tracks, true);
  else if (id === 'playlist') {
    const box = anchor.getBoundingClientRect();
    if (playlists.canAdd(tracks)) playlists.pick(tracks, box.left, box.top - 8, anchor);
  } else p.remove?.();
}
