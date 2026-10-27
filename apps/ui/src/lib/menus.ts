import { albumKey } from './albums';
import type { ViewState } from './context';
import type { MenuItem } from './contextMenu.svelte';
import type { EngineClient } from './engine.svelte';
import type { TrackRef } from './protocol/TrackRef';
import { inSelection, ordered } from './selection';
import type { TrackListData } from './sources.svelte';
import { toasts } from './toast.svelte';

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toasts.show('Copied');
  } catch {
    toasts.show('Could not copy', 'error');
  }
}

const inDesktop = () => '__TAURI_INTERNALS__' in window;

async function showInFolder(path: string) {
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('show_in_folder', { path }).catch((e: unknown) => toasts.show(String(e), 'error'));
}

/** Copy items for a track: title, "artist – title" and, for a local file, its path. */
function copyItems(track: TrackRef): MenuItem[] {
  const title = track.title ?? '';
  const items: MenuItem[] = [];
  if (title) items.push({ label: 'Copy title', run: () => copy(title) });
  if (title && track.artist) items.push({ label: 'Copy artist – title', run: () => copy(`${track.artist} – ${title}`) });
  if (track.source === 'local') items.push({ label: 'Copy path', run: () => copy(track.id) });
  return items;
}

/** Makes these tracks the context and plays the first. */
export function playTracks(engine: EngineClient, tracks: TrackRef[]) {
  if (tracks.length) engine.run({ type: 'set_context', tracks, start: 0, kind: 'list', label: null });
}

/** Queues tracks next or at the end; `tracks` may have to be fetched first. */
export async function queueTracks(engine: EngineClient, tracks: TrackRef[] | Promise<TrackRef[]>, next = false) {
  try {
    const list = await tracks;
    if (list.length) engine.run({ type: next ? 'enqueue_next' : 'enqueue', tracks: list });
  } catch (e) {
    engine.error = (e as Error).message;
  }
}

/** Indices of the picked rows of a list, for removing them from the queue. */
function pickedRows(data: TrackListData, view: ViewState): number[] {
  const rows = [];
  for (let i = 0; i < data.count; i++) if (view.browse.picked.has(data.key(i))) rows.push(i);
  return rows;
}

/** Removes the picked rows from the queue, or row `i` alone if it isn't part of a selection. */
export function removeRows(data: TrackListData, view: ViewState, i: number) {
  if (!data.remove) return;
  // Indices refer to the list as it is now, so the order of removal doesn't matter.
  for (const row of inSelection(view.browse.picked, data.key(i)) ? pickedRows(data, view) : [i]) data.remove(row);
}

/** The menu of row `i` in a track list or the queue; of the whole selection when the row is part of one. */
export function trackMenu(engine: EngineClient, data: TrackListData, view: ViewState, i: number): MenuItem[] {
  const track = data.track(i);
  if (!track) return [];
  if (inSelection(view.browse.picked, data.key(i))) {
    if (data.remove) return [{ label: 'Remove from queue', run: () => removeRows(data, view, i) }];
    const tracks = ordered(view.browse.picked);
    return [
      { label: 'Play', run: () => playTracks(engine, tracks) },
      { label: 'Queue next', run: () => queueTracks(engine, tracks, true) },
      { label: 'Add to queue', run: () => queueTracks(engine, tracks) },
    ];
  }
  const items: MenuItem[] = [{ label: 'Play', run: () => data.play(i) }];
  if (data.kind !== 'queue') {
    items.push({ label: 'Queue next', run: () => engine.run({ type: 'enqueue_next', tracks: [track] }) });
    items.push({ label: 'Add to queue', run: () => engine.run({ type: 'enqueue', tracks: [track] }) });
  }
  if (track.album) {
    const artist = data.details?.(i)?.album_artist ?? track.artist ?? '';
    items.push({ label: 'Go to album', run: () => (view.browse.focusAlbum = albumKey({ ...track, artist })) });
  }
  if (data.remove) items.push({ label: 'Remove from queue', run: () => removeRows(data, view, i) });
  if (track.source === 'local' && inDesktop()) items.push({ label: 'Show in folder', run: () => showInFolder(track.id) });
  return [...items, ...copyItems(track)];
}

/**
 * The menu of an album card, or of `count` picked albums. `tracks` fetches
 * the tracks the menu acts on.
 */
export function albumMenu(
  engine: EngineClient,
  album: { title: string; artist: string },
  play: () => void,
  tracks: () => Promise<TrackRef[]>,
  count = 1,
): MenuItem[] {
  const items: MenuItem[] = [
    { label: 'Play', run: play },
    { label: 'Queue next', run: () => queueTracks(engine, tracks(), true) },
    { label: 'Add to queue', run: () => queueTracks(engine, tracks()) },
  ];
  if (count > 1) return items;
  if (album.title) items.push({ label: 'Copy album title', run: () => copy(album.title) });
  if (album.title && album.artist) items.push({ label: 'Copy artist – album', run: () => copy(`${album.artist} – ${album.title}`) });
  return items;
}
