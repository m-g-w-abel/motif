import { describe, expect, it } from 'vitest';
import { GESTURES, HOLD, Press, SLOP, SWIPE, dragValue } from './gestures';

describe('Press', () => {
  it('picks no axis for small movements', () => {
    const p = new Press(100, 100, 0);
    expect(p.move(100 + SLOP - 2, 100 + 2)).toBeNull();
  });

  it('picks the axis the movement mostly runs along, and keeps it', () => {
    const x = new Press(100, 100, 0);
    expect(x.move(130, 110)).toBe('x');
    expect(x.move(130, 200)).toBe('x');
    const y = new Press(100, 100, 0);
    expect(y.move(105, 70)).toBe('y');
    expect(y.move(300, 70)).toBe('y');
  });

  it('becomes a long-press once held still long enough', () => {
    const p = new Press(0, 0, 1000);
    expect(p.held(1000 + HOLD - 1)).toBe(false);
    p.move(3, 3);
    expect(p.held(1000 + HOLD)).toBe(true);
  });

  it('cancels the long-press once it moves', () => {
    const p = new Press(0, 0, 0);
    p.move(0, SLOP + 1);
    expect(p.held(HOLD * 2)).toBe(false);
  });

  it('swipes left or right past the threshold', () => {
    const left = new Press(200, 100, 0);
    left.move(150, 100);
    expect(left.swipe(200 - SWIPE)).toBe('swipe-left');
    const right = new Press(200, 100, 0);
    right.move(250, 100);
    expect(right.swipe(200 + SWIPE)).toBe('swipe-right');
  });

  it('does not swipe when short or vertical', () => {
    const short = new Press(200, 100, 0);
    short.move(220, 100);
    expect(short.swipe(200 + SWIPE - 1)).toBeNull();
    const vertical = new Press(200, 100, 0);
    vertical.move(200, 40);
    expect(vertical.swipe(300)).toBeNull();
    expect(new Press(0, 0, 0).swipe(0)).toBeNull();
  });
});

describe('dragValue', () => {
  it('rises as the finger moves up and stays within 0..1', () => {
    expect(dragValue(0.5, -100, 200)).toBe(1);
    expect(dragValue(0.5, -50, 200)).toBe(0.75);
    expect(dragValue(0.5, 50, 200)).toBe(0.25);
    expect(dragValue(0.5, 400, 200)).toBe(0);
  });
});

describe('GESTURES', () => {
  it('has the decided defaults', () => {
    expect(GESTURES.bar).toEqual({ 'swipe-left': 'next', 'swipe-right': 'previous' });
    expect(GESTURES.artwork).toEqual({ 'swipe-left': 'next', 'swipe-right': 'previous', 'drag-vertical': 'volume' });
    expect(GESTURES.row).toEqual({ 'long-press': 'queue' });
  });
});
