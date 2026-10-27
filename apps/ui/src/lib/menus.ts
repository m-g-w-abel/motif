import { albumKey } from './albums';
import type { ViewState } from './context';
import type { MenuItem } from './contextMenu.svelte';
import type { EngineClient } from './engine.svelte';
import type { QueueItem } from './protocol/QueueItem';
import type { TrackRef } from './protocol/TrackRef';
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

function queueItems(engine: EngineClient, track: TrackRef): MenuItem[] {
  return [
    { label: 'Queue next', run: () => engine.run({ type: 'enqueue_next', tracks: [track] }) },
    { label: 'Add to queue', run: () => engine.run({ type: 'enqueue', tracks: [track] }) },
  ];
}

/** The menu of row `i` in a track list. */
export function trackMenu(engine: EngineClient, data: TrackListData, view: ViewState, i: number): MenuItem[] {
  const track = data.track(i);
  if (!track) return [];
  const items: MenuItem[] = [{ label: 'Play', run: () => data.play(i) }, ...queueItems(engine, track)];
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
  else items.push(...queueItems(engine, track));
  if (track.source === 'local' && inDesktop()) items.push({ label: 'Show in folder', run: () => showInFolder(track.id) });
  return [...items, ...copyItems(track)];
}

/** The menu of an album card. */
export function albumMenu(album: { title: string; artist: string }, play: () => void): MenuItem[] {
  const items: MenuItem[] = [{ label: 'Play', run: play }];
  if (album.title) items.push({ label: 'Copy album title', run: () => copy(album.title) });
  if (album.title && album.artist) items.push({ label: 'Copy artist – album', run: () => copy(`${album.artist} – ${album.title}`) });
  return items;
}
