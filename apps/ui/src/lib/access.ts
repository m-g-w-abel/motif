import type { Command } from './protocol/Command';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { Preset } from './protocol/Preset';
import type { Scope } from './protocol/Scope';

/** The scope each command needs, as the engine checks it (`required_scope` in crates/server). */
export const COMMAND_SCOPES: Record<Command['type'], Scope> = {
  play: 'playback',
  pause: 'playback',
  toggle_playback: 'playback',
  stop: 'playback',
  next: 'playback',
  previous: 'playback',
  seek: 'playback',
  set_volume: 'playback',
  set_repeat: 'playback',
  set_shuffle: 'playback',
  set_context: 'playback',
  play_item: 'playback',
  enqueue: 'queue',
  enqueue_next: 'queue',
  remove_item: 'queue',
  move_item: 'queue',
  clear_queue: 'queue',
  set_decoders: 'settings',
  set_output: 'settings',
  list_outputs: 'settings',
  set_replay_gain: 'settings',
};

/** The scope each library query needs; `null` for reads, which every client may do. */
const QUERY_SCOPES: Record<LibraryQuery['type'], Scope | null> = {
  status: null,
  tracks: null,
  groups: null,
  albums: null,
  recently_played: null,
  recently_played_albums: null,
  album_position: null,
  lookup: null,
  album_tracks: null,
  waveform: null,
  lyrics: null,
  playlists: null,
  playlist_tracks: null,
  play: 'playback',
  play_album: 'playback',
  play_playlist: 'playback',
  add_root: 'library',
  remove_root: 'library',
  rescan: 'library',
  set_watching: 'library',
  save_lyrics: 'library',
  create_playlist: 'library',
  rename_playlist: 'library',
  delete_playlist: 'library',
  add_to_playlist: 'library',
  remove_from_playlist: 'library',
  move_playlist_item: 'library',
};

export const queryScope = (query: LibraryQuery): Scope | null => QUERY_SCOPES[query.type];

/** The presets offered when approving a device, with their names. */
export const PRESETS: [Preset, string][] = [
  ['full', 'Full'],
  ['control_playback', 'Control playback'],
  ['view_only', 'View only'],
];

/** The preset a paired device's scopes match (`Preset::scopes` in crates/protocol), if any. */
export function presetOf(scopes: Scope[]): Preset | null {
  const has = (s: Scope) => scopes.includes(s);
  if (scopes.length === 5) return 'full';
  if (scopes.length === 2 && has('playback') && has('queue')) return 'control_playback';
  if (scopes.length === 0) return 'view_only';
  return null;
}
