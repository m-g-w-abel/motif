import type { ConnectionStatus, EngineClient } from './engine.svelte';
import type { Command } from './protocol/Command';
import type { PlaybackStatus } from './protocol/PlaybackStatus';
import type { PlayerState } from './protocol/PlayerState';

/** What the phone's media notification shows for a remote engine (`MainActivity.setRemoteMedia`). */
export type RemoteMedia = {
  title: string | null;
  artist: string | null;
  album: string | null;
  duration_ms: number | null;
  /** An image URL the phone can fetch. */
  cover: string | null;
  status: Exclude<PlaybackStatus, 'stopped'>;
  position_ms: number;
};

/** The notification for a remote engine, or `null` for none: only while connected and playing or paused. */
export function remoteMedia(
  connection: ConnectionStatus,
  player: PlayerState | null,
  positionMs: number,
  cover: string | null,
): RemoteMedia | null {
  const track = player?.current?.track;
  if (connection !== 'open' || !player || player.status === 'stopped' || !track) return null;
  const { title, artist, album, duration_ms } = track;
  return { title, artist, album, duration_ms, cover, status: player.status, position_ms: positionMs };
}

const android = (globalThis as { MotifAndroid?: { setRemoteMedia?(json: string | null): void } }).MotifAndroid;
const hook = globalThis as { motifRemoteButton?: (command: Command) => void };
/** Only the Android app has a media notification to show a remote engine in. */
export const canShowRemoteMedia = typeof android?.setRemoteMedia === 'function';
// A reload keeps the activity: without this, the notification stays with nothing to update it.
android?.setRemoteMedia?.(null);

/** Shows `media` in the phone's media notification and lock screen; `null` hands them back to the phone's engine. */
export function showRemoteMedia(media: RemoteMedia | null) {
  android?.setRemoteMedia?.(media && JSON.stringify(media));
}

/** Sends the notification's and headset's buttons to `engine` while it's shown. Returns the undo, which hides it. */
export function routeRemoteButtons(engine: EngineClient): () => void {
  // From our own Kotlin: the same commands the phone's engine gets from them.
  hook.motifRemoteButton = (command) => engine.run(command);
  return () => {
    delete hook.motifRemoteButton;
    showRemoteMedia(null);
  };
}
