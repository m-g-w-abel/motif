import { describe, expect, it } from 'vitest';
import { cantReach, deviceMark, deviceRows, elsewhere, pageMark } from './devices';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { PairedDevice } from './protocol/PairedDevice';
import type { PairingState } from './protocol/PairingState';
import type { Reach } from './protocol/Reach';

const track = (path: string, devices: string[]) =>
  ({ track: { source: 'local', id: path, title: 'Song' }, devices }) as unknown as LibraryTrack;
const here = track('/music/a.flac', ['k-here']);
const away = track('remote:k-laptop:7', ['k-laptop']);
const both = track('remote:k-laptop:8', ['k-desk', 'k-laptop']);

const pairing = (reach: Record<string, Reach>) =>
  ({
    engine_key: 'k-here',
    devices: [
      { id: 'd1', name: 'Laptop', public_key: 'k-laptop' },
      { id: 'd2', name: 'Desk', public_key: 'k-desk' },
      { id: 'd3', name: 'Phone', public_key: null },
    ],
    net: { links: Object.entries(reach).map(([device, reach]) => ({ device, reach })) },
  }) as unknown as PairingState;

describe('tracks on other devices', () => {
  it('are those from a linked engine’s catalog', () => {
    expect(elsewhere(here)).toBe(false);
    expect(elsewhere(away)).toBe(true);
  });

  it('can’t be reached while no device holding them is', () => {
    expect(cantReach(away, pairing({ d1: 'unreachable' }))).toBe('Laptop');
    expect(cantReach(away, pairing({}))).toBe('Laptop');
    expect(cantReach(both, pairing({ d1: 'unreachable', d2: 'unreachable' }))).toBe('Desk or Laptop');
  });

  it('can be reached when any one is, directly or relayed', () => {
    expect(cantReach(away, pairing({ d1: 'direct' }))).toBeNull();
    expect(cantReach(both, pairing({ d1: 'unreachable', d2: 'relayed' }))).toBeNull();
  });

  it('name a device no longer paired vaguely', () => {
    expect(cantReach(track('remote:k-gone:1', ['k-gone']), pairing({}))).toBe('another device');
  });

  it('are never called unreachable when here, or when reach is unknown', () => {
    expect(cantReach(here, pairing({}))).toBeNull();
    expect(cantReach(away, null)).toBeNull();
    expect(cantReach(undefined, pairing({}))).toBeNull();
  });

  it('get a mark naming where they are, dimmed while unreachable', () => {
    expect(deviceMark(here, pairing({}))).toBeNull();
    expect(deviceMark(away, pairing({ d1: 'direct' }))).toEqual({ title: 'On Laptop', away: false });
    expect(deviceMark(away, pairing({}))).toEqual({ title: 'On Laptop, which can’t be reached now', away: true });
    expect(deviceMark(away, null)).toEqual({ title: 'On another device', away: false });
  });

  it('mark a page when any of its songs is elsewhere, dimmed while none of those can be reached', () => {
    const desk = track('remote:k-desk:2', ['k-desk']);
    expect(pageMark([here, undefined], pairing({}))).toBeNull();
    expect(pageMark([here, away, both], pairing({ d1: 'direct' }))).toEqual({ title: 'Some songs are on Laptop or Desk', away: false });
    expect(pageMark([away, desk], pairing({ d2: 'relayed' }))).toEqual({ title: 'Some songs are on Laptop or Desk', away: false });
    expect(pageMark([here, away], pairing({}))).toEqual({ title: 'Some songs are on Laptop, which can’t be reached now', away: true });
  });
});

describe('deviceRows', () => {
  const saved = (id: string, name: string) => ({ id, name, addresses: ['1.2.3.4:7700'], token: 't' });
  const dev = (id: string, key: string | null, scopes: PairedDevice['scopes']) =>
    ({ id, name: id, public_key: key, scopes, paired_at_ms: 0, last_seen_ms: null }) as PairedDevice;

  it('joins a linked engine you also control into one row', () => {
    const rows = deviceRows([saved('k1', 'Laptop')], [dev('d1', 'k1', ['sync', 'playback'])], [{ device: 'd1', reach: 'relayed' }]);
    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ key: 'k1', browser: false, linked: true, reach: 'relayed' });
    expect(rows[0].saved).not.toBeNull();
    expect(rows[0].device).not.toBeNull();
  });

  it('lists a browser client by its device, marked as one', () => {
    const [row] = deviceRows([], [dev('d2', null, ['playback'])], []);
    expect(row).toMatchObject({ key: 'd2', browser: true, linked: false, saved: null, reach: null });
  });

  it('lists an engine you control that doesn’t know you, with no reach to tell', () => {
    const [row] = deviceRows([saved('k3', 'Desk')], [], []);
    expect(row).toMatchObject({ key: 'k3', device: null, linked: false, reach: null });
    expect(row.saved).not.toBeNull();
  });

  it('lists a device that controls you that you don’t control', () => {
    const [row] = deviceRows([], [dev('d4', 'k4', ['playback'])], [{ device: 'd4', reach: 'direct' }]);
    expect(row).toMatchObject({ key: 'k4', saved: null, linked: false, reach: 'direct' });
  });

  it('takes reach only from links: a paired device with none isn’t offline', () => {
    const [row] = deviceRows([], [dev('d5', 'k5', ['playback'])], []);
    expect(row.reach).toBeNull();
  });
});
