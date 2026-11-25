import { describe, expect, it } from 'vitest';
import type { EngineClient } from './engine.svelte';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { PairingState } from './protocol/PairingState';
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

  it('report a song on a device that can’t be reached instead of playing it', () => {
    const away = { source: 'local', id: 'remote:k1:4', title: 'Far' } as TrackRef;
    const details = { track: away, devices: ['k1'] } as unknown as LibraryTrack;
    const pairing = { devices: [{ id: 'd1', name: 'Laptop', public_key: 'k1' }], net: { links: [] } } as unknown as PairingState;
    const client = { current: null, pairing, error: null } as unknown as EngineClient;
    const played: number[] = [];
    const data = fixedData(client, 'Album', [{ track }, { track: away, details }], (i) => void played.push(i), '');
    data.play(1);
    expect(played).toEqual([]);
    expect(client.error).toBe('“Far” is on Laptop, which can’t be reached now.');
    data.play(0);
    expect(played).toEqual([0]);
  });
});
