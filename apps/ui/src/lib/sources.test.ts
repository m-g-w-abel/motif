import { describe, expect, it } from 'vitest';
import type { EngineClient } from './engine.svelte';
import type { TrackRef } from './protocol/TrackRef';
import { fixedData } from './sources.svelte';

const track = { source: 'local', id: '/music/a.flac', title: 'A' } as TrackRef;
const engine = { current: null } as unknown as EngineClient;

describe('fixed song lists', () => {
  it('key a playlist row by its entry, so a song in it twice is two rows', () => {
    const data = fixedData(engine, 'Mix', [{ track, item: 'op/0' }, { track, item: 'op/1' }], () => {}, '');
    expect([data.key(0), data.key(1)]).toEqual(['op/0', 'op/1']);
  });

  it("key an album's rows by track", () => {
    expect(fixedData(engine, 'Album', [{ track }], () => {}, '').key(0)).toBe(track.id);
  });
});
