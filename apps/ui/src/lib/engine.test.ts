import { describe, expect, it } from 'vitest';
import { PlaybackPosition, type EngineClient } from './engine.svelte';
import type { PlayerState } from './protocol/PlayerState';

describe('PlaybackPosition', () => {
  const state = (status: PlayerState['status'], sampledAgoMs: number, clockOffset: number) => {
    const player: PlayerState = {
      status,
      volume: 1,
      repeat: 'off',
      position_ms: 10_000,
      sampled_at_ms: Date.now() + clockOffset - sampledAgoMs,
      current: 0,
      decoders: [],
      available_decoders: [],
    };
    const engine = { player, clockOffset, current: null } as unknown as EngineClient;
    const position = new PlaybackPosition(engine);
    position.track()?.(); // one tick to read the clock, then stop
    return position.ms;
  };

  it('counts from when the engine took the snapshot, not when it arrived', () => {
    // Connected a minute into the track, with the engine's clock 5 s ahead of ours.
    expect(state('playing', 60_000, 5_000)).toBeCloseTo(70_000, -2);
  });

  it('holds still while paused', () => {
    expect(state('paused', 60_000, 0)).toBe(10_000);
  });
});
