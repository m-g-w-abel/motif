import { describe, expect, it } from 'vitest';
import { cantReach, deviceMark, elsewhere } from './devices';
import type { LibraryTrack } from './protocol/LibraryTrack';
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
});
