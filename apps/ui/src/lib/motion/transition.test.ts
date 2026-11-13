import { afterEach, describe, expect, it } from 'vitest';
import { motion } from './stage.svelte';
import { CURVE, DURATION, Transition, type Track } from './transition';

describe('CURVE', () => {
  it('runs from 0 to 1, and its inverse undoes it', () => {
    expect(CURVE.at(0)).toBe(0);
    expect(CURVE.at(1)).toBe(1);
    for (const p of [0.1, 0.25, 0.5, 0.9]) expect(CURVE.at(CURVE.inverse(p))).toBeCloseTo(p, 4);
  });

  it('is eased out: most of the way is done in the first half of the time', () => {
    expect(CURVE.at(0.5)).toBeGreaterThan(0.8);
  });
});

/** Just what Transition uses of a Web Animation. */
class Fake {
  currentTime: number | null = 0;
  playbackRate = 1;
  playState = 'running';
  log: string[] = [];
  finished: Promise<void>;
  finish!: () => void;
  cancelled = false;
  constructor(
    readonly frames: Keyframe[],
    readonly timing: KeyframeAnimationOptions,
  ) {
    this.finished = new Promise((r) => (this.finish = r));
  }
  pause = () => {
    this.playState = 'paused';
    this.log.push('pause');
  };
  play = () => {
    this.playState = 'running';
    this.log.push(`play ${this.playbackRate}`);
  };
  cancel = () => {
    this.cancelled = true;
  };
}

const made: Fake[] = [];
const el = () => ({ animate: (f: Keyframe[], t: KeyframeAnimationOptions) => (made.push(new Fake(f, t)), made.at(-1)) }) as unknown as HTMLElement;
const track = (span?: [number, number]): Track => ({ el: el(), frames: [{ opacity: 0 }, { opacity: 1 }], span });
const settle = () => new Promise((r) => setTimeout(r));

afterEach(() => {
  made.length = 0;
  Transition.hold = null;
});

describe('Transition', () => {
  it('lays each track over its span of the clock, paused', () => {
    new Transition([track(), track([0.5, 1])]);
    const [a, b] = made;
    expect(a.timing).toMatchObject({ delay: 0, duration: DURATION, fill: 'both' });
    const start = CURVE.inverse(0.5) * DURATION;
    expect(b.timing.delay).toBeCloseTo(start);
    expect(b.timing.duration).toBeCloseTo(DURATION - start);
    expect(a.log).toEqual(['pause']);
  });

  it('follows the finger: seeking draws exactly that progress, through the curve', () => {
    const t = new Transition([track()]);
    t.seek(0.3);
    expect(made[0].currentTime).toBeCloseTo(CURVE.inverse(0.3) * DURATION);
    expect(t.progress).toBeCloseTo(0.3);
    t.seek(2);
    expect(t.progress).toBe(1);
  });

  it('plays on from where it is, forwards or backwards, and says so once all have finished', async () => {
    const t = new Transition([track(), track()]);
    t.seek(0.4);
    let done = 0;
    void t.release(false).then(() => done++);
    expect(made.map((a) => a.log.at(-1))).toEqual(['play -1', 'play -1']);
    made[0].finish();
    await settle();
    expect(done).toBe(0);
    made[1].finish();
    await settle();
    expect(done).toBe(1);
  });

  it('finishes at once when let go already at its end (a swipe all the way), instead of playing again', async () => {
    let cleaned = 0;
    const t = new Transition([track()], () => cleaned++);
    t.seek(1);
    await t.release(true);
    expect(made[0].log).not.toContain('play 1');
    expect(cleaned).toBe(1);
    expect(t.progress).toBe(1);
    const u = new Transition([track()]);
    u.seek(0);
    await u.release(false);
    expect(made[1].log).not.toContain('play -1');
  });

  it('is safe to release twice: the second does nothing', async () => {
    let cleaned = 0;
    const t = new Transition([track()], () => cleaned++);
    const one = t.release(true);
    const two = t.release(false);
    expect(made[0].log).toEqual(['pause', 'play 1']);
    made[0].finish();
    await Promise.all([one, two]);
    expect(cleaned).toBe(1);
    expect(made[0].cancelled).toBe(true);
  });

  it('takes a late track where the others are, and plays it if they play', () => {
    const t = new Transition([track()]);
    t.seek(0.5);
    t.add(track());
    expect(made[1].currentTime).toBeCloseTo(made[0].currentTime!);
    void t.play(1);
    t.add(track());
    expect(made[2].log.at(-1)).toBe('play 1');
  });

  it('jumps to where it was heading when ended early, and cleans up once', async () => {
    let cleaned = 0;
    const t = new Transition([track()], () => cleaned++);
    void t.play(1);
    t.end();
    t.end();
    await settle();
    expect(t.progress).toBe(1);
    expect(cleaned).toBe(1);
  });

  it('is caught by a new finger where it is drawn, and then ends only as that finger lets go', async () => {
    let cleaned = 0;
    const t = new Transition([track()], () => cleaned++);
    let done = 0;
    void t.play(1).then(() => done++);
    made[0].currentTime = CURVE.inverse(0.6) * DURATION;
    expect(t.grab()).toBeCloseTo(0.6);
    expect(made[0].log.at(-1)).toBe('pause');
    expect(t.heading).toBeNull();
    // The play it caught finishing late changes nothing.
    made[0].finish();
    await settle();
    expect(cleaned).toBe(0);
    t.seek(0.2);
    expect(t.progress).toBeCloseTo(0.2);
    void t.release(false);
    expect(made[0].log.at(-1)).toBe('play -1');
    made[0].finish();
    await settle();
    expect(cleaned).toBe(1);
    expect(t.heading).toBe(0);
    expect(done).toBe(1);
    expect(t.grab()).toBeNull();
  });

  it('holds the next transition at the dev scrubber’s progress instead of playing it', () => {
    Transition.hold = 0.3;
    const t = new Transition([track()]);
    void t.play(1);
    expect(t.progress).toBeCloseTo(0.3);
    expect(made[0].log.at(-1)).toBe('pause');
    // Only the next one.
    const u = new Transition([track()]);
    void u.play(1);
    expect(made[1].log.at(-1)).toBe('play 1');
  });
});

describe('motion', () => {
  it('goes forward into a deeper screen, back out of one, and swaps between screens at the same depth', () => {
    expect(motion(1, 2)).toBe('forward');
    expect(motion(3, 2)).toBe('back');
    expect(motion(2, 2)).toBe('swap');
  });
});
