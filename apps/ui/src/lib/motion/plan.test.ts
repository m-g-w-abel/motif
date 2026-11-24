import { describe, expect, it } from 'vitest';
import { onScreen, plan, type Part } from './plan';

const box = (left: number, top: number, width = 100, height = 50) => ({ left, top, width, height });
const STAGE = box(0, 0, 400, 800);
/** A part: `el` is its element (the same on both sides: kept). */
const part = (el: string, b: ReturnType<typeof box>, more: Partial<Part> = {}): Part => ({ el, box: b, ...more });

describe('plan', () => {
  it('flies a name from one element to another, and moves a kept element from its old place', () => {
    const before = [part('mini title', box(10, 10), { name: 'title' }), part('controls', box(0, 500))];
    const after = [part('big title', box(20, 300), { name: 'title' }), part('controls', box(0, 600))];
    const p = plan(before, after, STAGE);
    expect(p.fly).toEqual([{ before: 0, after: 0 }]);
    expect(p.place).toEqual([{ after: 1, frames: [{ map: { sx: 1, sy: 1, x: 0, y: -100 } }, { map: { sx: 1, sy: 1, x: 0, y: 0 } }] }]);
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
    expect(q.fly).toEqual([{ before: 0, after: 1 }]);
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

  it('takes spans from the table for what enters or leaves (the declared name, else its kind, else all of it); moves always run the whole way', () => {
    const spans = {
      move: [0.1, 0.85],
      late: [0.4, 0.85],
      enter: [0.6, 1],
      fall: [0, 0.5],
    } as Record<string, [number, number]>;
    const before = [
      part('bar title', box(0, 700), { name: 'title', span: 'late' }),
      part('controls', box(0, 720)),
      part('buttons', box(0, 750), { enter: 'below', span: 'fall' }),
      part('sep', box(0, 0), { enter: 'fade' }),
    ];
    const after = [part('title', box(0, 300), { name: 'title', span: 'late' }), part('controls', box(0, 500)), part('format', box(0, 400), { enter: 'rise' })];
    const p = plan(before, after, STAGE, spans);
    // Every moving element starts at the start: a move has no span to delay it.
    expect(p.fly).toEqual([{ before: 0, after: 0 }]);
    expect(p.place.map((m) => 'span' in m)).toEqual([false]);
    expect(p.enter[0].span).toEqual([0.6, 1]);
    expect(p.leave.map((l) => l.span)).toEqual([
      [0, 0.5],
      [0, 1],
    ]);
  });

  it('moves the containers between parts too (implicit, kept only), so what they hold moves instead of jumping', () => {
    // The transport row holds play (a part) and the skip buttons (not parts): it moved up 100 with them.
    const before = [part('details', box(0, 400), { enter: 'rise' }), part('row', box(0, 600, 400, 50), { implicit: true, parent: 0 }), part('play', box(150, 600), { name: 'play', parent: 1 })];
    const after = [part('details', box(0, 400), { enter: 'rise' }), part('row', box(0, 500, 400, 50), { implicit: true, parent: 0 }), part('play', box(150, 500), { name: 'play', parent: 1 })];
    const p = plan(before, after, STAGE);
    expect(p.place.map((m) => [m.after, m.frames[0].map.y])).toEqual([[1, 100]]);
    // One that is new or gone neither comes nor goes: only declared parts do.
    const q = plan([part('old row', box(0, 0), { implicit: true })], [part('new row', box(0, 0), { implicit: true })], STAGE);
    expect([q.enter, q.leave, q.place, q.fly]).toEqual([[], [], [], []]);
  });

  it('carries a part that moves out of a box that clips it as a copy, so it is never cut off on the way', () => {
    const clip = (top: number) => part('h1', box(0, top, 200, 40), { implicit: true, clips: true });
    // The title moves 300 up inside its clipping line, which stays: it would leave it.
    const before = [clip(500), part('title', box(0, 500, 200, 40), { name: 'title', parent: 0 })];
    const after = [clip(500), part('title', box(0, 200, 200, 40), { name: 'title', parent: 0 })];
    const p = plan(before, after, STAGE);
    expect(p.fly).toEqual([{ before: 1, after: 1 }]);
    expect(p.place).toEqual([]);
    // The line moving with it, or a move that stays inside it: in place.
    expect(plan([clip(500), before[1]], [clip(200), after[1]], STAGE).fly).toEqual([]);
    // A container between parts (implicit) moving out of it is clipped by it, as at rest, not copied whole.
    const list = [clip(500), part('list', box(0, 500, 200, 40), { implicit: true, parent: 0 })];
    expect(plan(list, [clip(500), part('list', box(0, 200, 200, 40), { implicit: true, parent: 0 })], STAGE).place.map((m) => m.after)).toEqual([1]);
    const wide = (top: number) => part('body', box(0, 0, 400, 800), { implicit: true, clips: true });
    expect(plan([wide(0), before[1]], [wide(0), after[1]], STAGE).place.map((m) => m.after)).toEqual([1]);
  });

  it('says how far things move, for a finger to drive it by', () => {
    const p = plan([part('title', box(0, 700), { name: 'title' })], [part('t', box(0, 300), { name: 'title' }), part('q', box(0, 600), { enter: 'below' })], STAGE);
    expect(p.distance).toBe(400);
    expect(plan([], [part('x', box(0, 0), { enter: 'fade' })], STAGE).distance).toBe(100);
  });

  it('says a box lies on screen only when it meets every box that clips it', () => {
    const view = box(0, 0, 400, 800);
    expect(onScreen(box(10, 100), [view])).toBe(true);
    // Scrolled out of its scroller, above, below or beside it.
    const scroller = box(0, 100, 400, 300);
    expect(onScreen(box(10, 20), [view, scroller])).toBe(false);
    expect(onScreen(box(10, 450), [view, scroller])).toBe(false);
    expect(onScreen(box(500, 150), [view, scroller])).toBe(false);
    // Partly in view counts: it flies from what shows of it.
    expect(onScreen(box(10, 80), [view, scroller])).toBe(true);
    // Below the window, though inside a scroller that is itself taller.
    expect(onScreen(box(10, 900), [view, box(0, 0, 400, 2000)])).toBe(false);
  });
});
