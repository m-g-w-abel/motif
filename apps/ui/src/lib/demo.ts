import type { TrackInfo } from './protocol/TrackInfo';

/**
 * Stand-in library until library scanning lands in phase 1. The tracks are
 * fictional and have no audio; the engine only simulates their playback.
 */
const albums: { artist: string; album: string; ext: string; tracks: [string, number][] }[] = [
  {
    artist: 'Halcyon Drift',
    album: 'Signals',
    ext: 'flac',
    tracks: [['Low Orbit', 214], ['Paper Lanterns', 187], ['Cartographer', 263], ['Static Bloom', 198], ['Return Path', 305]],
  },
  {
    artist: 'Mira Vale',
    album: 'Night Transit',
    ext: 'opus',
    tracks: [['Sodium Lights', 241], ['Glasshouse', 12], ['Last Train Home', 276], ['Overpass', 222]],
  },
  {
    artist: 'The Quiet Engines',
    album: 'Machine Hymns',
    ext: 'dsf',
    tracks: [['Prelude in Copper', 158], ['Assembly Line', 247], ['Governor', 312], ['Coda', 189]],
  },
  {
    artist: 'Ostra',
    album: 'Tidewater',
    ext: 'mp3',
    tracks: [['Brine', 201], ['Salt Marsh', 233], ['Undertow', 284], ['Lighthouse Keeper', 259], ['Ebb', 175]],
  },
];

export const demoLibrary: TrackInfo[] = albums.flatMap(({ artist, album, ext, tracks }) =>
  tracks.map(([title, seconds], i) => ({
    uri: `file:///demo/${encodeURIComponent(album)}/${String(i + 1).padStart(2, '0')}.${ext}`,
    title,
    artist,
    album,
    duration_ms: seconds * 1000,
  })),
);
