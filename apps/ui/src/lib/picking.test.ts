import { describe, expect, it } from 'vitest';
import { pickActions, type Picking } from './picking';

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

  it('disables the song actions until something is picked, but never All or Add to playlist', () => {
    const { bar, more } = pickActions(picking({ count: 0, remove: () => {} }), every);
    const disabled = [...bar, ...more].filter((a) => a.disabled).map((a) => a.id);
    expect(disabled).toEqual(['play', 'queue', 'next', 'delete']);
  });

  it('selects all, or none once everything is picked', () => {
    expect(pickActions(picking(), every).more.find((a) => a.id === 'all')?.label).toBe('Select all');
    expect(pickActions(picking({ all: true }), every).more.find((a) => a.id === 'all')?.label).toBe('Select none');
  });
});
