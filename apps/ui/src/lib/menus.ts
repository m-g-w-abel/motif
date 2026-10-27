import { albumKey } from './albums';
import { trackTitle } from './format';
import type { ViewState } from './context';
import type { MenuItem } from './contextMenu.svelte';
import type { EngineClient } from './engine.svelte';
import type { QueueItem } from './protocol/QueueItem';
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

/**
 * Queues tracks next or at the end and says so; `tracks` may have to be
 * fetched first. `label` names them in the toast (an album), else the
 * track's title or the count.
 */
export async function queueTracks(
  engine: EngineClient,
  tracks: TrackRef[] | Promise<TrackRef[]>,
  next = false,
  label?: string,
) {
  try {
    const list = await tracks;
    if (!list.length) return;
    engine.run({ type: next ? 'enqueue_next' : 'enqueue', tracks: list });
    const what = label ?? (list.length === 1 ? trackTitle(list[0]) : `${list.length} tracks`);
    toasts.show(`${next ? 'Queued next' : 'Added to queue'}: ${what}`);
  } catch (e) {
    engine.error = (e as Error).message;
  }
}

function queueItems(engine: EngineClient, tracks: TrackRef[]): MenuItem[] {
  return [
    { label: 'Queue next', run: () => queueTracks(engine, tracks, true) },
    { label: 'Add to queue', run: () => queueTracks(engine, tracks) },
  ];
}

/** The menu of row `i` in a track list; of the whole selection when the row is part of one. */
export function trackMenu(engine: EngineClient, data: TrackListData, view: ViewState, i: number): MenuItem[] {
  const track = data.track(i);
  if (!track) return [];
  if (inSelection(view.browse.picked, data.key(i))) {
    const tracks = ordered(view.browse.picked);
    return [{ label: 'Play', run: () => playTracks(engine, tracks) }, ...queueItems(engine, tracks)];
  }
  const items: MenuItem[] = [{ label: 'Play', run: () => data.play(i) }, ...queueItems(engine, [track])];
  if (track.album) {
    const artist = data.details?.(i)?.album_artist ?? track.artist ?? '';
    items.push({ label: 'Go to album', run: () => (view.browse.focusAlbum = albumKey({ ...track, artist })) });
  }
  if (track.source === 'local' && inDesktop()) items.push({ label: 'Show in folder', run: () => showInFolder(track.id) });
  return [...items, ...copyItems(track)];
}

/** The menu of an entry under Up next: one in the queue (`queued`) or coming from the context. */
export function queueMenu(engine: EngineClient, item: QueueItem, queued: boolean): MenuItem[] {
  const { track } = item;
  const items: MenuItem[] = [{ label: 'Play', run: () => engine.run({ type: 'play_item', item_id: item.id }) }];
  if (queued) items.push({ label: 'Remove from queue', run: () => engine.run({ type: 'remove_item', item_id: item.id }) });
  else items.push(...queueItems(engine, [track]));
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
  const label = count > 1 ? `${count} albums` : album.title || 'Unknown album';
  const items: MenuItem[] = [
    { label: 'Play', run: play },
    { label: 'Queue next', run: () => queueTracks(engine, tracks(), true, label) },
    { label: 'Add to queue', run: () => queueTracks(engine, tracks(), false, label) },
  ];
  if (count > 1) return items;
  if (album.title) items.push({ label: 'Copy album title', run: () => copy(album.title) });
  if (album.title && album.artist) items.push({ label: 'Copy artist – album', run: () => copy(`${album.artist} – ${album.title}`) });
  return items;
}
