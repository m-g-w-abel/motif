import { describe, expect, it } from 'vitest';
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
    list.sync();
    await settle();
    expect(queries.filter((q) => q.type === 'tracks')).toHaveLength(2);
    expect(queries.every((q) => !('search' in q) || q.search === 'x')).toBe(true);
  });
});
