import { describe, expect, it } from 'vitest';
import { plan, type Part } from './plan';

const box = (left: number, top: number, width = 100, height = 50) => ({ left, top, width, height });
const STAGE = box(0, 0, 400, 800);
/** A part: `el` is its element (the same on both sides: kept). */
const part = (el: string, b: ReturnType<typeof box>, more: Partial<Part> = {}): Part => ({ el, box: b, ...more });

describe('plan', () => {
  it('flies a name from one element to another, and moves a kept element from its old place', () => {
    const before = [part('mini title', box(10, 10), { name: 'title' }), part('controls', box(0, 500))];
    const after = [part('big title', box(20, 300), { name: 'title' }), part('controls', box(0, 600))];
    const p = plan(before, after, STAGE);
    expect(p.fly).toEqual([{ before: 0, after: 0, span: [0, 1] }]);
    expect(p.place).toEqual([{ after: 1, frames: [{ map: { sx: 1, sy: 1, x: 0, y: -100 } }, { map: { sx: 1, sy: 1, x: 0, y: 0 } }], span: [0, 1] }]);
    expect(p.enter).toEqual([]);
    expect(p.leave).toEqual([]);
  });

  it('leaves a kept element that stayed put alone', () => {
    const p = plan([part('a', box(0, 0), { enter: 'fade' })], [part('a', box(0, 0), { enter: 'fade' })], STAGE);
    expect(p.place).toEqual([]);
    expect(p.enter).toEqual([]);
    expect(p.leave).toEqual([]);
  });

  it('brings in what is only after as it declares, and takes out what is only before the same way, mirrored', () => {
    const p = plan([part('details', box(0, 400), { enter: 'rise' })], [part('queue', box(0, 300), { enter: 'below' })], STAGE);
    expect(p.enter).toEqual([{ after: 0, frames: [{ map: { sx: 1, sy: 1, x: 0, y: 500 } }, { map: { sx: 1, sy: 1, x: 0, y: 0 } }], span: [0, 1] }]);
    expect(p.leave).toEqual([{ before: 0, frames: [{ map: { sx: 1, sy: 1, x: 0, y: 0 }, opacity: 1 }, { map: { sx: 1, sy: 1, x: 0, y: 40 }, opacity: 0 }], span: [0, 1] }]);
  });

  it('enters from each side of the stage, fades, and grows about its centre', () => {
    const at = box(100, 200, 200, 100);
    const away = (enter: Part['enter']) => plan([], [part('x', at, { enter })], STAGE).enter[0].frames[0];
    expect(away('above').map.y).toBe(-300);
    expect(away('right').map.x).toBe(300);
    expect(away('left').map.x).toBe(-300);
    expect(away('fade')).toEqual({ map: { sx: 1, sy: 1, x: 0, y: 0 }, opacity: 0 });
    const grow = away('grow');
    expect(grow.opacity).toBe(0);
    // The centre stays where it is.
    expect(grow.map.sx * 100 + grow.map.x).toBeCloseTo(100);
    expect(grow.map.sy * 50 + grow.map.y).toBeCloseTo(50);
  });

  it('fades a name with no counterpart that declares nothing', () => {
    const p = plan([part('album', box(0, 0), { name: 'album' })], [], STAGE);
    expect(p.leave[0].frames[1].opacity).toBe(0);
  });

  it('leaves what is inside something coming or going to it: it rides along', () => {
    const before = [part('details', box(0, 400), { enter: 'rise' }), part('more', box(0, 700), { enter: 'fade', parent: 0 })];
    const after = [part('mini', box(0, 0), { enter: 'above' }), part('names', box(60, 10), { name: 'artist', parent: 0 })];
    const p = plan(before, after, STAGE);
    expect(p.leave.map((l) => l.before)).toEqual([0]);
    expect(p.enter.map((e) => e.after)).toEqual([0]);
    // A name inside something entering still flies, when it has a counterpart.
    const q = plan([part('artist', box(0, 300), { name: 'artist' })], after, STAGE);
    expect(q.fly).toEqual([{ before: 0, after: 1, span: [0, 1] }]);
  });

  it('carries nothing inside a flight: its destination is hidden while a copy travels', () => {
    const before = [part('frame', box(0, 0, 400, 400), { name: 'cover' })];
    const after = [part('strip art', box(0, 0, 48, 48), { name: 'cover' }), part('badge', box(0, 0, 10, 10), { enter: 'fade', parent: 0 })];
    expect(plan(before, after, STAGE).enter).toEqual([]);
  });

  it('moves a kept part inside a kept part that moved by only its own share (the lift)', () => {
    const before = [part('controls', box(0, 500)), part('play', box(150, 520), { name: 'play', parent: 0 }), part('time', box(0, 510), { enter: 'fade', parent: 0 })];
    const after = [part('controls', box(0, 300)), part('play', box(150, 300), { name: 'play', parent: 0 }), part('time', box(0, 310), { enter: 'fade', parent: 0 })];
    const p = plan(before, after, STAGE);
    // The controls moved 200 up; play moved 220, so 20 of its own; the time moved with them.
    expect(p.place.map((m) => [m.after, m.frames[0].map.y])).toEqual([
      [0, 200],
      [1, 20],
    ]);
  });

  it('takes spans from the table: the declared name, else its kind, else all of it', () => {
    const spans = { move: [0.1, 0.85], enter: [0.6, 1], fall: [0, 0.5] } as Record<string, [number, number]>;
    const before = [part('bar title', box(0, 700), { name: 'title' }), part('buttons', box(0, 750), { enter: 'below', span: 'fall' }), part('sep', box(0, 0), { enter: 'fade' })];
    const after = [part('title', box(0, 300), { name: 'title' }), part('format', box(0, 400), { enter: 'rise' })];
    const p = plan(before, after, STAGE, spans);
    expect(p.fly[0].span).toEqual([0.1, 0.85]);
    expect(p.enter[0].span).toEqual([0.6, 1]);
    expect(p.leave.map((l) => l.span)).toEqual([[0, 0.5], [0, 1]]);
  });

  it('says how far things move, for a finger to drive it by', () => {
    const p = plan([part('title', box(0, 700), { name: 'title' })], [part('t', box(0, 300), { name: 'title' }), part('q', box(0, 600), { enter: 'below' })], STAGE);
    expect(p.distance).toBe(400);
    expect(plan([], [part('x', box(0, 0), { enter: 'fade' })], STAGE).distance).toBe(100);
  });
});
