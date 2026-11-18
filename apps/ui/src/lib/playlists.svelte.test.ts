import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { moveCommand, Playlists } from './playlists.svelte';
import type { Playlist } from './protocol/Playlist';
import type { TrackRef } from './protocol/TrackRef';

describe('playlists', () => {
  const items = ['a', 'b', 'c'];
  it('names a drag by the entry it lands before, so a repeat lands it in the same place', () => {
    expect(moveCommand(items, 0, 2)).toEqual({ item: 'a', before: null });
    expect(moveCommand(items, 2, 0)).toEqual({ item: 'c', before: 'a' });
    expect(moveCommand(items, 0, 1)).toEqual({ item: 'a', before: 'c' });
  });
});

describe('playlist choices', () => {
  const engine = () => ({ query: vi.fn(async () => ({ type: 'ok' })), can: () => true }) as unknown as EngineClient;
  const track = { source: 'local', id: '/a.flac' } as TrackRef;
  const list = [{ id: 'p1', name: 'Road' }] as Playlist[];

  it('offers a new playlist, then each playlist', () => {
    const playlists = new Playlists(engine());
    playlists.list = list;
    expect(playlists.choices([track]).map((c) => c.label)).toEqual(['New playlist…', 'Road']);
  });

  it('runs `then` once the songs are added, not when a new playlist is cancelled', async () => {
    const e = engine();
    const playlists = new Playlists(e);
    playlists.list = list;
    const then = vi.fn();
    const [make, road] = playlists.choices([track], then);
    make.run();
    playlists.answer(null);
    await vi.waitFor(() => expect(playlists.asking).toBeNull());
    await Promise.resolve();
    expect(then).not.toHaveBeenCalled();
    road.run();
    await vi.waitFor(() => expect(then).toHaveBeenCalledOnce());
    make.run();
    playlists.answer('Night');
    await vi.waitFor(() => expect(then).toHaveBeenCalledTimes(2));
    expect(e.query).toHaveBeenCalledWith(expect.objectContaining({ type: 'create_playlist', name: 'Night', paths: ['/a.flac'] }));
  });
});
