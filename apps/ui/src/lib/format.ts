import type { Streamed } from './protocol/Streamed';
import type { TrackRef } from './protocol/TrackRef';

export function formatTime(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Title for display, falling back to the file name in the track's id. */
export function trackTitle(track: TrackRef): string {
  if (track.title) return track.title;
  const name = track.id.split(/[/\\]/).pop() ?? track.id;
  return name.replace(/\.[^.]+$/, '') || track.id;
}

/** Upper-case container/codec hint from the file extension, e.g. `FLAC`. */
export function trackFormat(track: TrackRef): string {
  const match = /\.([a-z0-9]+)$/i.exec(track.id);
  return match ? match[1].toUpperCase() : '';
}

/**
 * What is playing, as the file info shows it: a transcoded stream as the Opus that arrives, a streamed one with its
 * device (and route off the LAN). A song `elsewhere` (on linked engines) shows nothing until it's known how it streams.
 */
export function playedFormat(
  file: { codec?: string | null; sample_rate?: number | null; bitrate?: number | null } | null | undefined,
  format: string,
  streamed: Streamed | null | undefined,
  elsewhere = false,
): { rate: string; bitrate: string; format: string; device: string } {
  if (elsewhere && !streamed) return { rate: '', bitrate: '', format: '', device: '' };
  const device = streamed ? `${streamed.from}${streamed.route === 'lan' ? '' : ` · ${streamed.route}`}` : '';
  if (streamed && streamed.sent !== 'original') {
    return { rate: '48 kHz', bitrate: `${streamed.sent.replace('opus', '')} kbps`, format: 'OPUS (transcoded)', device };
  }
  return {
    rate: file?.sample_rate ? `${file.sample_rate / 1000} kHz` : '',
    bitrate: file?.bitrate ? `${file.bitrate} kbps` : '',
    format,
    device,
  };
}

/** A stable two-colour gradient per album, used until real artwork exists. */
export function placeholderArt(track: TrackRef): string {
  const key = track.album ?? track.title ?? track.id;
  let hash = 0;
  for (const ch of key) hash = (hash * 31 + ch.codePointAt(0)!) | 0;
  // The golden angle spreads similar hashes far apart on the colour wheel.
  const hue = Math.round(((hash >>> 0) * 137.508) % 360);
  return `linear-gradient(135deg, hsl(${hue} 70% 58%), hsl(${(hue + 60) % 360} 65% 42%))`;
}
