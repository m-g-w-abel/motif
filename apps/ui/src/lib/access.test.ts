import { describe, expect, it } from 'vitest';
import { COMMAND_SCOPES, presetOf } from './access';
import type { EngineClient } from './engine.svelte';
import { albumMenu, queueMenu } from './menus';
import type { Preset } from './protocol/Preset';
import type { QueueItem } from './protocol/QueueItem';
import type { Scope } from './protocol/Scope';

/** `Preset::scopes` in crates/protocol. */
const SCOPES: Record<Preset, Scope[]> = {
  full: ['playback', 'queue', 'library', 'settings', 'pairing'],
  control_playback: ['playback', 'queue'],
  view_only: [],
};

const engineFor = (preset: Preset) => ({ can: (scope: Scope) => SCOPES[preset].includes(scope) }) as unknown as EngineClient;
const item: QueueItem = {
  id: 1,
  track: { source: 'demo', id: 'a', keys: {} as QueueItem['track']['keys'], title: 'A', artist: 'B', album: null, duration_ms: 1000 },
  error: null,
};
const labels = (preset: Preset) => ({
  queued: queueMenu(engineFor(preset), item, true).map((i) => i.label),
  album: albumMenu(engineFor(preset), { title: 'X', artist: 'Y' }, () => {}, async () => []).map((i) => i.label),
});

describe('presets', () => {
  it('hide what a device may not do', () => {
    expect(labels('full')).toEqual({
      queued: ['Play', 'Remove from queue', 'Copy title', 'Copy artist – title'],
      album: ['Play', 'Queue next', 'Add to queue', 'Copy album title', 'Copy artist – album'],
    });
    expect(labels('control_playback')).toEqual(labels('full'));
    expect(labels('view_only')).toEqual({
      queued: ['Copy title', 'Copy artist – title'],
      album: ['Copy album title', 'Copy artist – album'],
    });
  });

  it('are recognised from scopes', () => {
    for (const preset of Object.keys(SCOPES) as Preset[]) expect(presetOf(SCOPES[preset])).toBe(preset);
    expect(presetOf(['playback'])).toBe(null);
  });

  it('cover every command', () => {
    expect(COMMAND_SCOPES.toggle_playback).toBe('playback');
    expect(COMMAND_SCOPES.enqueue).toBe('queue');
  });
});
