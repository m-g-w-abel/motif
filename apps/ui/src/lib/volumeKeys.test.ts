import { describe, expect, it } from 'vitest';
import { stepVolume, volumeKeys } from './volumeKeys';

describe('stepVolume', () => {
  it('moves one of 15 steps', () => {
    expect(stepVolume(0, 1)).toBeCloseTo(1 / 15);
    expect(stepVolume(8 / 15, -1)).toBeCloseTo(7 / 15);
  });

  it('snaps an in-between volume to the next step that way', () => {
    expect(stepVolume(0.5, 1)).toBeCloseTo(8 / 15);
    expect(stepVolume(0.5, -1)).toBeCloseTo(7 / 15);
  });

  it('stays within 0 and 1', () => {
    expect(stepVolume(1, 1)).toBe(1);
    expect(stepVolume(0, -1)).toBe(0);
  });
});

describe('volumeKeys', () => {
  it('steps from the last sent volume while the engine has yet to answer', () => {
    const sent: number[] = [];
    let now = 0;
    const press = volumeKeys(() => 0, (v) => sent.push(v), () => now);
    press(1);
    now = 50;
    press(1);
    expect(sent.map((v) => Math.round(v * 15))).toEqual([1, 2]);
  });

  it("steps from the engine's volume after a pause", () => {
    const sent: number[] = [];
    let now = 0;
    let engine = 0;
    const press = volumeKeys(() => engine, (v) => sent.push(v), () => now);
    press(1);
    engine = 10 / 15; // changed elsewhere meanwhile
    now = 5000;
    press(1);
    expect(sent.map((v) => Math.round(v * 15))).toEqual([1, 11]);
  });
});
