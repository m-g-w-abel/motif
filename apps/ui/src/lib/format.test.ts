import { describe, expect, it } from 'vitest';
import { playedFormat } from './format';

const flac = { codec: 'FLAC', sample_rate: 44100, bitrate: 1600 };

describe('playedFormat', () => {
  it('shows a transcoded stream as the Opus that arrives, and its device', () => {
    expect(playedFormat(flac, 'FLAC', { from: 'Desktop', sent: 'opus256', route: 'lan' })).toEqual({
      rate: '48 kHz',
      bitrate: '256 kbps',
      format: 'OPUS (transcoded)',
      device: 'Desktop',
    });
  });

  it("shows an original stream as its file, and its device's route off the LAN", () => {
    expect(playedFormat(flac, 'FLAC', { from: 'Desktop', sent: 'original', route: 'relayed' })).toEqual({
      rate: '44.1 kHz',
      bitrate: '1600 kbps',
      format: 'FLAC',
      device: 'Desktop · relayed',
    });
    const internet = playedFormat(flac, 'FLAC', { from: 'Desktop', sent: 'opus128', route: 'internet' });
    expect(internet.device).toBe('Desktop · internet');
    expect(internet.bitrate).toBe('128 kbps');
  });

  it("shows nothing for a song from elsewhere until it's known how it streams", () => {
    expect(playedFormat(flac, 'FLAC', null, true)).toEqual({ rate: '', bitrate: '', format: '', device: '' });
    expect(playedFormat(flac, 'FLAC', { from: 'Desktop', sent: 'original', route: 'lan' }, true).format).toBe('FLAC');
  });

  it('shows a local file as it is, with no device', () => {
    expect(playedFormat(flac, 'FLAC', null)).toEqual({ rate: '44.1 kHz', bitrate: '1600 kbps', format: 'FLAC', device: '' });
    expect(playedFormat(null, 'MP3', undefined)).toEqual({ rate: '', bitrate: '', format: 'MP3', device: '' });
  });
});
