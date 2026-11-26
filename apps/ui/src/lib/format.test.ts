import { describe, expect, it } from 'vitest';
import { streamedLabel } from './format';

describe('streamedLabel', () => {
  it('names the engine, what it sends and the route', () => {
    expect(streamedLabel({ from: 'Desktop', sent: 'opus256', route: 'relayed' }, 'FLAC')).toBe(
      'Streamed from Desktop · Opus 256 (from FLAC) · relayed',
    );
    expect(streamedLabel({ from: 'Desktop', sent: 'original', route: 'lan' }, 'FLAC')).toBe(
      'Streamed from Desktop · original FLAC · LAN',
    );
    expect(streamedLabel({ from: 'Desktop', sent: 'opus128', route: 'internet' }, undefined)).toBe(
      'Streamed from Desktop · Opus 128 · internet',
    );
  });
});
