import { describe, expect, it } from 'vitest';
import type { PlayerState } from './protocol/PlayerState';
import type { QueueItem } from './protocol/QueueItem';
import { remoteMedia } from './remoteMedia';

const item: QueueItem = {
  id: 1,
  track: {
    source: 'local',
    id: '/music/a.flac',
    keys: {} as QueueItem['track']['keys'],
    title: 'Song',
    artist: 'Band',
    album: 'Record',
    duration_ms: 200_000,
  },
  error: null,
};

const player = (status: PlayerState['status'], current: QueueItem | null = item) =>
  ({ status, current, position_ms: 0 }) as PlayerState;

describe('remoteMedia', () => {
  it("shows the remote engine's track, state and position", () => {
    expect(remoteMedia('open', player('paused'), 42_000, 'http://host/art/x')).toEqual({
      title: 'Song',
      artist: 'Band',
      album: 'Record',
      duration_ms: 200_000,
      cover: 'http://host/art/x',
      status: 'paused',
      position_ms: 42_000,
    });
  });

  it('shows nothing while disconnected or reconnecting', () => {
    expect(remoteMedia('closed', player('playing'), 0, null)).toBeNull();
    expect(remoteMedia('connecting', player('playing'), 0, null)).toBeNull();
    expect(remoteMedia('open', null, 0, null)).toBeNull();
  });

  it('shows nothing when stopped or with nothing queued', () => {
    expect(remoteMedia('open', player('stopped'), 0, null)).toBeNull();
    expect(remoteMedia('open', player('paused', null), 0, null)).toBeNull();
  });
});
