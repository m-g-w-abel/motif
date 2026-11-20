import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { LibraryClient, LibraryTracks, RecentPlays } from './library.svelte';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { QueryResult } from './protocol/QueryResult';

/** An engine whose library has `total` tracks named by index. */
function fakeEngine(total: number) {
  const queries: LibraryQuery[] = [];
  const engine = {
    status: 'open',
    library: { revision: 1, plays: 1, ratings: 1 },
    query: async (q: LibraryQuery): Promise<QueryResult> => {
      queries.push(q);
      if (q.type === 'recently_played') return { type: 'played', total: 0, plays: [] } as unknown as QueryResult;
      if (q.type === 'set_favourite' || q.type === 'set_rating') return { type: 'done' };
      if (q.type === 'lookup') return { type: 'tracks', total: 1, offset: 0, tracks: [{ track: { source: 'local', id: '/a.flac' }, favourite: false, rating: null }] } as unknown as QueryResult;
      if (q.type === 'groups') return { type: 'groups', groups: [] };
      if (q.type !== 'tracks') throw new Error(q.type);
      const tracks = Array.from({ length: Math.max(0, Math.min(q.limit, total - q.offset)) }, (_, i) => ({
        track: { source: 'local', id: `/${q.offset + i}.flac` },
      }));
      return { type: 'tracks', total, offset: q.offset, tracks } as unknown as QueryResult;
    },
  };
  return { engine: engine as unknown as EngineClient & { library: { revision: number; plays: number; ratings: number } }, queries };
}

vi.useFakeTimers({ toFake: ['Date'] });
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('LibraryTracks', () => {
  it('fetches pages as rows are asked for, once each', async () => {
    const { engine, queries } = fakeEngine(1000);
    const list = new LibraryTracks(engine, null);
    list.sync();
    expect(list.get(450)).toBeUndefined();
    list.get(451);
    await settle();
    expect(list.total).toBe(1000);
    expect(list.get(450)?.track.id).toBe('/450.flac');
    const pages = queries.filter((q) => q.type === 'tracks').map((q) => (q as { offset: number }).offset);
    expect(pages.sort((a, b) => a - b)).toEqual([0, 400]);
  });

  it('refetches when the library changes', async () => {
    const { engine, queries } = fakeEngine(10);
    const list = new LibraryTracks(engine, 'x');
    list.sync();
    await settle();
    list.sync(); // same revision: nothing new
    engine.library.revision = 2;
    vi.advanceTimersByTime(1000);
    list.sync();
    await settle();
    expect(queries.filter((q) => q.type === 'tracks')).toHaveLength(2);
    expect(queries.every((q) => !('search' in q) || q.search === 'x')).toBe(true);
  });
});

describe('RecentPlays', () => {
  it('refetches on a new play, which tracks lists ignore', async () => {
    const { engine, queries } = fakeEngine(10);
    const [plays, tracks] = [new RecentPlays(engine, null), new LibraryTracks(engine, null)];
    plays.sync();
    tracks.sync();
    await settle();
    engine.library.plays = 2;
    vi.advanceTimersByTime(1000);
    plays.sync();
    tracks.sync();
    await settle();
    expect(queries.filter((q) => q.type === 'recently_played')).toHaveLength(2);
    expect(queries.filter((q) => q.type === 'tracks')).toHaveLength(1);
  });
});

describe('stale data', () => {
  it('keeps old rows until the refetch lands, and throttles refetches', async () => {
    const { engine, queries } = fakeEngine(10);
    const list = new LibraryTracks(engine, null);
    list.sync();
    await settle();
    expect(list.get(3)).toBeDefined();
    engine.library.revision = 2;
    list.sync(); // throttled: too soon after the first
    expect(queries.filter((q) => q.type === 'tracks')).toHaveLength(1);
    expect(list.get(3)).toBeDefined();
    vi.advanceTimersByTime(1000);
    list.sync();
    expect(list.get(3)).toBeDefined(); // stale row, while refetching
    await settle();
    expect(queries.filter((q) => q.type === 'tracks')).toHaveLength(2);
  });

  it('reports a shifted anchor row', async () => {
    const { engine } = fakeEngine(10);
    const list = new LibraryTracks(engine, null);
    list.sync();
    await settle();
    list.setAnchor(4);
    let moved = -1;
    list.onShift = (i) => (moved = i);
    // Same row ids at other indices after an insert above.
    const real = engine.query;
    engine.query = async (q) => {
      const r = await real(q);
      if (r.type === 'tracks') r.tracks = [{ track: { source: 'local', id: '/new.flac' } } as never, ...r.tracks.slice(0, 9)];
      return r;
    };
    engine.library.revision = 2;
    vi.advanceTimersByTime(1000);
    list.sync();
    await settle();
    expect(moved).toBe(5);
  });
});


describe('marking tracks', () => {
  const local = { source: 'local', id: '/a.flac' } as never;

  it('shows a mark at once and sends it by path', async () => {
    const { engine, queries } = fakeEngine(1);
    const library = new LibraryClient(engine);
    library.details(local);
    await settle();
    expect(library.details(local)).toMatchObject({ favourite: false, rating: null });
    library.setFavourite([local], true);
    library.setRating([local], 7);
    expect(library.details(local)).toMatchObject({ favourite: true, rating: 7 });
    expect(queries.filter((q) => q.type.startsWith('set_'))).toEqual([
      { type: 'set_favourite', paths: ['/a.flac'], favourite: true },
      { type: 'set_rating', paths: ['/a.flac'], rating: 7 },
    ]);
    library.setRating([local], null);
    expect(library.details(local)?.rating).toBeNull();
  });

  it('leaves tracks that are not local alone', () => {
    const { engine, queries } = fakeEngine(1);
    new LibraryClient(engine).setFavourite([{ source: 'other', id: 'x' } as never], true);
    expect(queries).toEqual([]);
  });
});
