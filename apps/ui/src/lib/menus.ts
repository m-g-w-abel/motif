import { albumKey } from './albums';
import { trackTitle } from './format';
import type { ViewState } from './context';
import type { MenuItem } from './contextMenu.svelte';
import type { EngineClient } from './engine.svelte';
import { addFolder } from './folders';
import type { QueueItem } from './protocol/QueueItem';
import type { TrackRef } from './protocol/TrackRef';
import { inSelection, ordered } from './selection';
import { launcherItems } from './launcher';
import { isAndroid } from './platform';
import type { Playlists } from './playlists.svelte';
import type { SongListData } from './sources.svelte';
import { toasts } from './toast.svelte';

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    toasts.show('Copied');
  } catch {
    toasts.show('Could not copy', 'error');
  }
}

/** A file manager to show files in: the desktop app's, never Android's. */
const inDesktop = () => '__TAURI_INTERNALS__' in window && !isAndroid;

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
  if (!engine.can('queue')) return;
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

/** Play, if this device may: `play` runs it. */
function playItem(engine: EngineClient, play: () => void): MenuItem[] {
  return engine.can('playback') ? [{ label: 'Play', run: play }] : [];
}

function queueItems(engine: EngineClient, tracks: TrackRef[]): MenuItem[] {
  if (!engine.can('queue')) return [];
  return [
    { label: 'Queue next', run: () => queueTracks(engine, tracks, true) },
    { label: 'Add to queue', run: () => queueTracks(engine, tracks) },
  ];
}

/**
 * The menu of row `i` in a track list; of the whole selection when the row is part of one.
 * `openAlbum` goes to the album's page; without it Go to album shows the album in the layout.
 */
export function trackMenu(
  engine: EngineClient,
  playlists: Playlists,
  data: SongListData,
  view: ViewState,
  i: number,
  openAlbum?: (key: string, title: string) => void,
): MenuItem[] {
  const track = data.track(i);
  if (!track) return [];
  if (inSelection(view.browse.picked, data.key(i))) {
    const tracks = ordered(view.browse.picked);
    return [...playItem(engine, () => playTracks(engine, tracks)), ...queueItems(engine, tracks), ...playlists.addItems(tracks)];
  }
  const items: MenuItem[] = [...playItem(engine, () => data.play(i)), ...queueItems(engine, [track]), ...playlists.addItems([track])];
  if (track.album) {
    const artist = data.details?.(i)?.album_artist ?? track.artist ?? '';
    const key = albumKey({ ...track, artist });
    items.push({ label: 'Go to album', run: () => (openAlbum ? openAlbum(key, track.album!) : (view.browse.focusAlbum = key)) });
  }
  if (track.source === 'local' && inDesktop()) items.push({ label: 'Show in folder', run: () => showInFolder(track.id) });
  return [...items, ...copyItems(track)];
}

/** The menu of an entry under Up next: one in the queue (`queued`) or coming from the context. */
export function queueMenu(engine: EngineClient, item: QueueItem, queued: boolean): MenuItem[] {
  const { track } = item;
  const items = playItem(engine, () => engine.run({ type: 'play_item', item_id: item.id }));
  if (queued && engine.can('queue')) items.push({ label: 'Remove from queue', run: () => engine.run({ type: 'remove_item', item_id: item.id }) });
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
  const items: MenuItem[] = [...playItem(engine, play)];
  if (engine.can('queue')) {
    items.push(
      { label: 'Queue next', run: () => queueTracks(engine, tracks(), true, label) },
      { label: 'Add to queue', run: () => queueTracks(engine, tracks(), false, label) },
    );
  }
  if (count > 1) return items;
  if (album.title) items.push({ label: 'Copy album title', run: () => copy(album.title) });
  if (album.title && album.artist) items.push({ label: 'Copy artist – album', run: () => copy(`${album.artist} – ${album.title}`) });
  return items;
}

/** The Library home's ⋮ menu. */
export function libraryMenu(engine: EngineClient): MenuItem[] {
  const rescan = () => engine.query({ type: 'rescan' }).catch((e: Error) => (engine.error = e.message));
  return [
    { label: 'Add folder…', run: () => void addFolder(engine) },
    { label: 'Rescan library', run: () => void rescan() },
    ...launcherItems({ kind: 'home' }),
  ];
}
