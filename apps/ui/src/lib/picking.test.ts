import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import { pickActions, runPick, type Picking } from './picking';
import type { Playlists } from './playlists.svelte';

const picking = (over: Partial<Picking> = {}): Picking => ({ count: 2, all: false, tracks: () => [], toggleAll: () => {}, ...over });
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
  const playlists = { canAdd: () => true, pick: vi.fn() } as unknown as Playlists;
  const anchor = { getBoundingClientRect: () => ({ left: 0, top: 0 }) } as HTMLElement;

  it('does nothing with nothing picked, except Select all', () => {
    const p = picking({ count: 0, toggleAll: vi.fn(), remove: vi.fn() });
    for (const id of ['play', 'queue', 'playlist', 'next', 'delete'] as const) runPick(id, p, engine, playlists, anchor);
    expect(engine.send).not.toHaveBeenCalled();
    expect(engine.run).not.toHaveBeenCalled();
    expect(playlists.pick).not.toHaveBeenCalled();
    expect(p.remove).not.toHaveBeenCalled();
    runPick('all', p, engine, playlists, anchor);
    expect(p.toggleAll).toHaveBeenCalledOnce();
  });

  it('runs an action on the picks', () => {
    const p = picking({ remove: vi.fn() });
    runPick('delete', p, engine, playlists, anchor);
    runPick('playlist', p, engine, playlists, anchor);
    expect(p.remove).toHaveBeenCalledOnce();
    expect(playlists.pick).toHaveBeenCalledOnce();
  });
});
