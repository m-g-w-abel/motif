import { describe, expect, it } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { hold, LibraryClient, type LibraryTracks, type Shown } from './library.svelte';
import type { QueryResult } from './protocol/QueryResult';

const engine = {
  status: 'open',
  library: { revision: 1 },
  query: async (q: { type: string }): Promise<QueryResult> =>
    q.type === 'groups'
      ? { type: 'groups', groups: [] }
      : ({ type: 'tracks', total: 1, offset: 0, tracks: [{ track: { source: 'local', id: '/a' } }] } as unknown as QueryResult),
} as unknown as EngineClient;
const title = [{ key: 'title', descending: false }] as never;
const settle = () => new Promise((r) => setTimeout(r, 0));

describe('holding the shown list', () => {
  it('swaps as soon as the new order has its first page, with nothing else changing', async () => {
    const client = new LibraryClient(engine);
    const shown: Shown<LibraryTracks> = {};
    let sort = $state<never[]>([]);
    const next = $derived(client.tracks(null, sort));
    const list = $derived(hold(next, shown));
    const first = list;
    first.sync();
    await settle();
    sort = title;
    expect(list).toBe(first); // held while the new order loads
    await settle();
    expect(list).not.toBe(first);
    expect(list.loaded).toBe(true);
  });

  it('keeps separate callers apart', () => {
    const client = new LibraryClient(engine);
    const a: Shown<LibraryTracks> = {};
    const b: Shown<LibraryTracks> = {};
    const la = hold(client.tracks(null, []), a);
    expect(hold(client.tracks('x', []), b)).not.toBe(la);
    expect(hold(client.tracks('x', []), a)).toBe(la); // a still holds the first list
  });
});
