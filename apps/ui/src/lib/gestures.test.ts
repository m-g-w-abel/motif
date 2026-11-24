import { afterEach, describe, expect, it } from 'vitest';
import { CHOICES, GESTURES, HOLD, Press, SLOP, SWIPE, dragValue, gesture, mapped, type Swipe } from './gestures';

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
  it('follows the finger sideways only once the press is a horizontal drag, from where it became one (no jump by the slop)', () => {
    const p = new Press(200, 100, 0);
    expect(p.offset(205)).toBe(0);
    p.move(170, 104);
    expect(p.offset(170)).toBe(0);
    expect(p.offset(150)).toBe(-20);
    expect(p.offset(260)).toBe(90);
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
    expect(GESTURES.artwork).toEqual({ 'swipe-left': 'next', 'swipe-right': 'previous', 'drag-vertical': 'close' });
    expect(GESTURES.row).toEqual({ 'long-press': 'select', 'swipe-left': 'queue' });
    expect(GESTURES.queued).toEqual({ 'swipe-left': 'remove' });
    expect(GESTURES.view).toEqual({ 'swipe-right': 'back', 'swipe-left': 'forward' });
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
    expect(mapped({ artwork: { 'drag-vertical': 'volume' } }, 'artwork')['drag-vertical']).toBe('volume');
  });

  it('makes a long press on a row select only: a saved remap to a queue action falls back to the default', () => {
    expect(CHOICES.row['long-press']).toEqual(['select']);
    expect(mapped({ row: { 'long-press': 'queue' } }, 'row')).toEqual({ 'long-press': 'select', 'swipe-left': 'queue' });
    expect(mapped({ row: { 'long-press': 'play-next', 'swipe-left': 'play-next' } }, 'row')).toEqual({ 'long-press': 'select', 'swipe-left': 'play-next' });
  });

  it('drops a gesture mapped to none', () => {
    expect(mapped({ bar: { 'swipe-left': 'none' } }, 'bar')).toEqual({ 'swipe-right': 'previous' });
    expect(mapped({ row: { 'long-press': 'none', 'swipe-left': 'none' } }, 'row')).toEqual({});
    expect(mapped({ view: { 'swipe-right': 'none', 'swipe-left': 'none' } }, 'view')).toEqual({});
    expect(mapped({ queued: { 'swipe-left': 'none' } }, 'queued')).toEqual({});
  });

  it('ignores stored overrides the gesture does not offer, and garbage', () => {
    const bad = { row: { 'long-press': 'volume', 'swipe-left': 'next' }, view: { 'swipe-right': 'next', 'swipe-left': 'back' }, bar: { 'long-press': 'queue' }, artwork: 'x' };
    expect(mapped(bad, 'row')).toEqual(GESTURES.row);
    expect(mapped(bad, 'bar')).toEqual(GESTURES.bar);
    expect(mapped(bad, 'artwork')).toEqual(GESTURES.artwork);
    expect(mapped(bad, 'view')).toEqual(GESTURES.view);
    expect(mapped(null, 'bar')).toEqual(GESTURES.bar);
    expect(mapped('x', 'bar')).toEqual(GESTURES.bar);
  });
});

describe('gesture swipes', () => {
  /** A window with just listeners, to send a finger's moves through. */
  const listeners = new Map<string, Set<(e: unknown) => void>>();
  (globalThis as { window?: unknown }).window = {
    addEventListener: (type: string, f: (e: unknown) => void) => listeners.set(type, (listeners.get(type) ?? new Set()).add(f)),
    removeEventListener: (type: string, f: (e: unknown) => void) => listeners.get(type)?.delete(f),
  };
  const send = (type: string, clientX: number) => {
    for (const f of [...(listeners.get(type) ?? [])]) f({ type, pointerId: 1, clientX, clientY: 0, cancelable: false, preventDefault() {} });
  };
  afterEach(() => listeners.clear());
  const press = { pointerType: 'touch', isPrimary: true, pointerId: 1, clientX: 0, clientY: 0 } as PointerEvent;

  /** Swipes from 0 through `xs`, letting go at the last; what the drawn swipe was asked. */
  function swipe(map: Parameters<typeof gesture>[1], xs: number[], end = 'pointerup') {
    const log: string[] = [];
    const shown: Swipe = (way) => (log.push(`build ${way}`), { distance: 100, drive: { seek: (p) => log.push(`seek ${p}`), release: (c) => log.push(`release ${c}`) } });
    const ran: string[] = [];
    gesture(press, map, { next: () => ran.push('next'), queue: () => ran.push('queue') }, shown);
    for (const x of xs) send('pointermove', x);
    send(end, xs.at(-1)!);
    return { log, ran };
  }

  it('drives the way that does something with the finger, from where it became a swipe, and completes it when the swipe does', () => {
    const { log, ran } = swipe({ 'swipe-left': 'next' }, [-SLOP, -30, -80]);
    expect(log).toEqual(['build -1', 'seek 0.2', 'seek 0.7', 'release true']);
    expect(ran).toEqual(['next']);
  });

  it("springs back when the swipe doesn't get far enough, goes back past its start, or is cancelled", () => {
    expect(swipe({ 'swipe-left': 'next' }, [-SLOP, -30]).log.at(-1)).toBe('release false');
    expect(swipe({ 'swipe-left': 'next' }, [-SLOP, -40, 20, 90]).log.slice(-2)).toEqual(['seek 0', 'release false']);
    expect(swipe({ 'swipe-left': 'next' }, [-SLOP, -80], 'pointercancel').log.at(-1)).toBe('release false');
  });

  it('draws nothing the way that does nothing (a row swiped right)', () => {
    expect(swipe({ 'swipe-left': 'queue' }, [SLOP, 80]).log).toEqual([]);
  });
});
