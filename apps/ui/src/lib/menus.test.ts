import { describe, expect, it, vi } from 'vitest';
import type { EngineClient } from './engine.svelte';
import type { LibraryClient } from './library.svelte';
import { markItems } from './menus';
import type { TrackRef } from './protocol/TrackRef';

const track = { source: 'local', id: '/a.flac' } as TrackRef;
const engine = (library = true) => ({ can: () => library }) as unknown as EngineClient;
const library = () => ({ setFavourite: vi.fn(), setRating: vi.fn() });
const labels = (items: { label: string }[]) => items.map((i) => i.label);

describe('markItems', () => {
  it('loves and unloves in hearts mode', () => {
    const lib = library();
    const [love] = markItems(engine(), lib as unknown as LibraryClient, 'hearts', [track], { favourite: false, rating: 6 });
    expect(love.label).toBe('Love');
    love.run();
    expect(lib.setFavourite).toHaveBeenCalledWith([track], true);
    const [unlove] = markItems(engine(), lib as unknown as LibraryClient, 'hearts', [track], { favourite: true, rating: null });
    expect(unlove.label).toBe('Unlove');
  });

  it('only clears a rating in stars mode, and only when there is one', () => {
    const lib = library();
    expect(markItems(engine(), lib as unknown as LibraryClient, 'stars', [track], { favourite: true, rating: null })).toEqual([]);
    const items = markItems(engine(), lib as unknown as LibraryClient, 'stars', [track], { favourite: false, rating: 8 });
    expect(labels(items)).toEqual(['Clear rating']);
    items[0].run();
    expect(lib.setRating).toHaveBeenCalledWith([track], null);
  });

  it('offers nothing without library access or local tracks', () => {
    const other = { source: 'other', id: 'x' } as TrackRef;
    expect(markItems(engine(false), library() as unknown as LibraryClient, 'hearts', [track])).toEqual([]);
    expect(markItems(engine(), library() as unknown as LibraryClient, 'hearts', [other])).toEqual([]);
  });
});
