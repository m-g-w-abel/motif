import { describe, expect, it } from 'vitest';
import { CHOICES, GESTURES, HOLD, Press, SLOP, SWIPE, dragValue, mapped } from './gestures';

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

describe('Press.offset', () => {
  it('follows the finger sideways only once the press is a horizontal drag', () => {
    const p = new Press(200, 100, 0);
    expect(p.offset(205)).toBe(0);
    p.move(170, 104);
    expect(p.offset(170)).toBe(-30);
    expect(p.offset(260)).toBe(60);
    const v = new Press(200, 100, 0);
    v.move(204, 60);
    expect(v.offset(260)).toBe(0);
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

describe('CHOICES', () => {
  it('offers every default gesture, with its default among its actions', () => {
    for (const [surface, map] of Object.entries(GESTURES)) {
      for (const [g, action] of Object.entries(map)) {
        expect(CHOICES[surface as keyof typeof CHOICES][g as keyof typeof map]).toContain(action);
      }
    }
  });
});

describe('mapped', () => {
  it('is the defaults with no overrides', () => {
    expect(mapped({}, 'artwork')).toEqual(GESTURES.artwork);
    expect(mapped({}, 'row')).toEqual(GESTURES.row);
  });

  it('applies an override and keeps the other gestures', () => {
    expect(mapped({ artwork: { 'drag-vertical': 'seek' } }, 'artwork')).toEqual({
      'swipe-left': 'next',
      'swipe-right': 'previous',
      'drag-vertical': 'seek',
    });
    expect(mapped({ bar: { 'swipe-left': 'previous', 'swipe-right': 'next' } }, 'bar')).toEqual({
      'swipe-left': 'previous',
      'swipe-right': 'next',
    });
    expect(mapped({ row: { 'long-press': 'play-next' } }, 'row')).toEqual({ 'long-press': 'play-next' });
  });

  it('drops a gesture mapped to none', () => {
    expect(mapped({ bar: { 'swipe-left': 'none' } }, 'bar')).toEqual({ 'swipe-right': 'previous' });
    expect(mapped({ row: { 'long-press': 'none' } }, 'row')).toEqual({});
  });

  it('ignores stored overrides the gesture does not offer, and garbage', () => {
    const bad = { row: { 'long-press': 'volume', 'swipe-left': 'next' }, bar: { 'long-press': 'queue' }, artwork: 'x' };
    expect(mapped(bad, 'row')).toEqual(GESTURES.row);
    expect(mapped(bad, 'bar')).toEqual(GESTURES.bar);
    expect(mapped(bad, 'artwork')).toEqual(GESTURES.artwork);
    expect(mapped(null, 'bar')).toEqual(GESTURES.bar);
    expect(mapped('x', 'bar')).toEqual(GESTURES.bar);
  });
});
