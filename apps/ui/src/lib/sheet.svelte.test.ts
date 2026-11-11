import { describe, expect, it } from 'vitest';
import { PlayerSheet, STEP, released, settling, stepTo } from './sheet.svelte';

function opened(step?: 'half' | 'full' | 'lyrics') {
  const sheet = new PlayerSheet();
  sheet.expand();
  if (step === 'lyrics') sheet.toggleLyrics();
  if (step === 'half' || step === 'full') sheet.up();
  if (step === 'full') sheet.up();
  return sheet;
}

describe('PlayerSheet steps', () => {
  it('opens on now playing', () => {
    const sheet = opened();
    expect(sheet.open).toBe(true);
    expect(sheet.step).toBe('now');
  });

  it('steps up from now playing to the half queue, then the full queue, and stops there', () => {
    const sheet = opened();
    sheet.up();
    expect(sheet.step).toBe('half');
    sheet.up();
    expect(sheet.step).toBe('full');
    sheet.up();
    expect(sheet.step).toBe('full');
  });

  it('steps back full → half → now playing → closed', () => {
    const sheet = opened('full');
    sheet.back();
    expect(sheet.step).toBe('half');
    sheet.back();
    expect(sheet.step).toBe('now');
    expect(sheet.open).toBe(true);
    sheet.back();
    expect(sheet.open).toBe(false);
  });

  it('goes from lyrics back to now playing, and up does nothing there', () => {
    const sheet = opened('lyrics');
    expect(sheet.lyrics).toBe(true);
    sheet.up();
    expect(sheet.step).toBe('lyrics');
    sheet.back();
    expect(sheet.step).toBe('now');
    expect(sheet.lyrics).toBe(false);
  });

  it('toggles lyrics from any step, and back to now playing', () => {
    const sheet = opened('half');
    sheet.toggleLyrics();
    expect(sheet.step).toBe('lyrics');
    sheet.toggleLyrics();
    expect(sheet.step).toBe('now');
  });

  it('reopens on now playing after closing from the queue, but keeps lyrics', () => {
    const queue = opened('full');
    queue.collapse();
    queue.expand();
    expect(queue.step).toBe('now');
    const lyrics = opened('lyrics');
    lyrics.collapse();
    lyrics.expand();
    expect(lyrics.step).toBe('lyrics');
  });

  it('ignores steps while closed', () => {
    const sheet = new PlayerSheet();
    sheet.up();
    sheet.back();
    expect(sheet.open).toBe(false);
    expect(sheet.step).toBe('now');
  });
});

describe('released', () => {
  it('is a tap without movement, a step past the threshold, and nothing in between', () => {
    expect(released(3)).toBe('tap');
    expect(released(-STEP)).toBe('up');
    expect(released(STEP + 20)).toBe('back');
    expect(released(STEP / 2)).toBeNull();
    expect(released(-STEP / 2)).toBeNull();
  });
});

describe('stepTo', () => {
  it('goes up towards the full queue and back towards now playing, then closed', () => {
    expect(stepTo('now', 'up')).toBe('half');
    expect(stepTo('half', 'up')).toBe('full');
    expect(stepTo('full', 'up')).toBeNull();
    expect(stepTo('lyrics', 'up')).toBeNull();
    expect(stepTo('full', 'back')).toBe('half');
    expect(stepTo('half', 'back')).toBe('now');
    expect(stepTo('lyrics', 'back')).toBe('now');
    expect(stepTo('now', 'back')).toBe('closed');
  });
});

describe('settling', () => {
  it('is where a settle has got to: from its start to its end over the settle, quickly at first', () => {
    expect(settling(0, 1, 0)).toBe(0);
    expect(settling(1, 0, 0)).toBe(1);
    expect(settling(0, 1, 1)).toBe(1);
    expect(settling(0, 1, 5)).toBe(1);
    const half = settling(0, 1, 0.5);
    expect(half).toBeGreaterThan(0.5);
    expect(half).toBeLessThan(1);
    expect(settling(1, 0, 0.5)).toBeCloseTo(1 - half);
  });
});
