import type { TrackRef } from './protocol/TrackRef';

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

const baseLibrary: TrackRef[] = albums.flatMap(({ artist, album, ext, tracks }) =>
  tracks.map(([title, seconds], i) => ({
    source: 'demo',
    id: `/demo/${album}/${String(i + 1).padStart(2, '0')}.${ext}`,
    keys: { musicbrainz_recording: null, isrc: null, audio_hash: null },
    title,
    artist,
    album,
    duration_ms: seconds * 1000,
  })),
);

// Development only: ?dev-library=50000 repeats the albums to stress-test long lists.
const devSize =
  import.meta.env.DEV && typeof location !== 'undefined'
    ? Number(new URLSearchParams(location.search).get('dev-library'))
    : 0;

export const demoLibrary: TrackRef[] = devSize
  ? Array.from({ length: devSize }, (_, i) => {
      const track = baseLibrary[i % baseLibrary.length];
      const copy = Math.floor(i / baseLibrary.length);
      return copy ? { ...track, id: `${track.id}?copy=${copy}`, album: `${track.album} ${copy + 1}` } : track;
    })
  : baseLibrary;
