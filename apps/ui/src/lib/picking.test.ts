import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { keyPicking, pickActions, runPick, type Picking } from './picking';
import type { TrackRef } from './protocol/TrackRef';
import { toggle, type ItemAt } from './selection';

const picking = (over: Partial<Picking> = {}): Picking => ({ count: 2, all: false, tracks: () => [], toggleAll: () => {}, done: () => {}, ...over });
const every = { playback: true, queue: true, playlist: true };

describe('pickActions', () => {
  it('puts Play, Add to queue and Add to playlist on the bar, the rest in its menu', () => {
    const { bar, more } = pickActions(picking({ remove: () => {} }), every);
    expect(bar.map((a) => a.id)).toEqual(['play', 'queue', 'playlist']);
    expect(more.map((a) => a.id)).toEqual(['next', 'all', 'delete']);
  });

  it('offers only what this device may do', () => {
    const { bar, more } = pickActions(picking(), { playback: false, queue: true, playlist: false });
    expect(bar.map((a) => a.id)).toEqual(['queue']);
    expect(more.map((a) => a.id)).toEqual(['next', 'all']);
  });

  it('shows every action even before anything is picked', () => {
    const { bar, more } = pickActions(picking({ count: 0, remove: () => {} }), every);
    expect([...bar, ...more].map((a) => a.id)).toEqual(['play', 'queue', 'playlist', 'next', 'all', 'delete']);
  });

  it('selects all, or none once everything is picked', () => {
    expect(pickActions(picking(), every).more.find((a) => a.id === 'all')?.label).toBe('Select all');
    expect(pickActions(picking({ all: true }), every).more.find((a) => a.id === 'all')?.label).toBe('Select none');
  });
});

describe('runPick', () => {
  const engine = { send: vi.fn(async () => {}), run: vi.fn(), can: () => true } as unknown as EngineClient;

  it('does nothing with nothing picked, except Select all', () => {
    const p = picking({ count: 0, toggleAll: vi.fn(), remove: vi.fn(), done: vi.fn() });
    for (const id of ['play', 'queue', 'next', 'delete'] as const) runPick(id, p, engine);
    expect(engine.send).not.toHaveBeenCalled();
    expect(engine.run).not.toHaveBeenCalled();
    expect(p.remove).not.toHaveBeenCalled();
    expect(p.done).not.toHaveBeenCalled();
    runPick('all', p, engine);
    expect(p.toggleAll).toHaveBeenCalledOnce();
  });

  it('leaves select mode after an action, but not after Select all', () => {
    const p = picking({ remove: vi.fn(), done: vi.fn() });
    runPick('all', p, engine);
    expect(p.done).not.toHaveBeenCalled();
    for (const id of ['play', 'queue', 'next', 'delete'] as const) runPick(id, p, engine);
    expect(p.remove).toHaveBeenCalledOnce();
    expect(p.done).toHaveBeenCalledTimes(4);
  });
});

describe('keyPicking', () => {
  const keys = ['a', 'b', 'c', 'd'];
  const at: ItemAt<string> = (i) => [keys[i], keys[i]];
  const song = (id: string) => ({ source: 'local', id }) as TrackRef;

  it('hands over the songs of the picked albums or playlists, pick after pick in list order', async () => {
    const picked = toggle(toggle(toggle(new Map(), at, 3), at, 0), at, 2);
    const songs = vi.fn(async (ks: string[]) => ks.flatMap((k) => [song(`${k}1`), song(`${k}2`)]));
    const p = keyPicking(picked, keys.length, songs, { toggleAll: () => {}, done: () => {} });
    expect(p.count).toBe(3);
    expect(p.all).toBe(false);
    expect((await p.tracks()).map((t) => t.id)).toEqual(['a1', 'a2', 'c1', 'c2', 'd1', 'd2']);
    expect(songs).toHaveBeenCalledWith(['a', 'c', 'd']);
  });

  it('runs an action once the songs have come', async () => {
    const engine = { run: vi.fn(), can: () => true } as unknown as EngineClient;
    const done = vi.fn();
    const p = keyPicking(toggle(new Map(), at, 1), keys.length, async () => [song('b1')], { toggleAll: () => {}, done });
    runPick('play', p, engine);
    await vi.waitFor(() => expect(engine.run).toHaveBeenCalledWith(expect.objectContaining({ type: 'set_context', tracks: [song('b1')] })));
    expect(done).toHaveBeenCalledOnce();
  });
});
