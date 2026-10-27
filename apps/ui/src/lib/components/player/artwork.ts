import type { TrackRef } from '../../protocol/TrackRef';

/** An image that has been decoded and is on screen, and the album it was for. */
export interface ShownArt {
  id: string;
  album: string;
}

/** Identifies an album by artist and title; `null` for tracks that have none (they never share an image). */
export function albumOf(track: TrackRef | null | undefined): string | null {
  return track?.album ? `${track.artist ?? ''}\0${track.album}` : null;
}

/**
 * The image to keep on screen for `track`: the one already shown while it is
 * from the same album, even if this track's own art is unknown or still
 * decoding (albums can have per-track art, so the id may still change). Another
 * album never keeps the old image.
 */
export function keptArt(shown: ShownArt | null, track: TrackRef | null | undefined): string | null {
  const album = albumOf(track);
  return shown && album !== null && shown.album === album ? shown.id : null;
}
