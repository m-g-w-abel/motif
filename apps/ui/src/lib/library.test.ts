import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { LibraryTracks } from './library.svelte';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { QueryResult } from './protocol/QueryResult';

/** An engine whose library has `total` tracks named by index. */
function fakeEngine(total: number) {
  const queries: LibraryQuery[] = [];
  const engine = {
    status: 'open',
    library: { revision: 1 },
    query: async (q: LibraryQuery): Promise<QueryResult> => {
      queries.push(q);
      if (q.type === 'groups') return { type: 'groups', groups: [] };
      if (q.type !== 'tracks') throw new Error(q.type);
      const tracks = Array.from({ length: Math.max(0, Math.min(q.limit, total - q.offset)) }, (_, i) => ({
        track: { source: 'local', id: `/${q.offset + i}.flac` },
      }));
      return { type: 'tracks', total, offset: q.offset, tracks } as unknown as QueryResult;
    },
  };
  return { engine: engine as unknown as EngineClient & { library: { revision: number } }, queries };
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
