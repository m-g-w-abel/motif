import type { TrackInfo } from './protocol/TrackInfo';

export function formatTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Title for display, falling back to the file name in the URI. */
export function trackTitle(track: TrackInfo): string {
  if (track.title) return track.title;
  const name = decodeURIComponent(track.uri.split('/').pop() ?? track.uri);
  return name.replace(/\.[^.]+$/, '') || track.uri;
}

/** Upper-case container/codec hint from the file extension, e.g. `FLAC`. */
export function trackFormat(track: TrackInfo): string {
  const match = /\.([a-z0-9]+)$/i.exec(track.uri);
  return match ? match[1].toUpperCase() : '';
}

/** A stable two-colour gradient per album, used until real artwork exists. */
export function placeholderArt(track: TrackInfo): string {
  const key = track.album ?? track.title ?? track.uri;
  let hash = 0;
  for (const ch of key) hash = (hash * 31 + ch.codePointAt(0)!) | 0;
  // The golden angle spreads similar hashes far apart on the colour wheel.
  const hue = Math.round(((hash >>> 0) * 137.508) % 360);
  return `linear-gradient(135deg, hsl(${hue} 70% 58%), hsl(${(hue + 60) % 360} 65% 42%))`;
}
