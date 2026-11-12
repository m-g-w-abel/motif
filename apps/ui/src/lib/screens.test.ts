import { describe, expect, it } from 'vitest';
import { Scrub } from './screens';

describe('Scrub', () => {
  /** Just what Scrub uses of a Web Animation. */
  class Fake {
    currentTime: number | null = null;
    log: string[] = [];
    finished: Promise<void>;
    finish!: () => void;
    constructor(readonly duration: number) {
      this.finished = new Promise((r) => (this.finish = r));
    }
    effect = { getComputedTiming: () => ({ duration: this.duration }) };
    pause = () => this.log.push('pause');
    play = () => this.log.push('play');
    reverse = () => this.log.push('reverse');
  }

  it('starts paused and follows the drag, each animation through its own length', () => {
    const [a, b] = [new Fake(300), new Fake(100)];
    const scrub = new Scrub([a, b] as unknown as Animation[]);
    scrub.seek(0.5);
    expect([a.currentTime, b.currentTime]).toEqual([150, 50]);
    scrub.seek(2);
    expect([a.currentTime, b.currentTime]).toEqual([300, 100]);
    expect(a.log).toEqual(['pause']);
  });

  it('plays on to the end, or back to the start, then says so once all have finished', async () => {
    const [a, b] = [new Fake(300), new Fake(300)];
    const scrub = new Scrub([a, b] as unknown as Animation[]);
    let done = 0;
    scrub.release(false, () => done++);
    expect([a.log, b.log]).toEqual([['pause', 'reverse'], ['pause', 'reverse']]);
    a.finish();
    await Promise.resolve();
    expect(done).toBe(0);
    b.finish();
    await new Promise((r) => setTimeout(r));
    expect(done).toBe(1);
    const on = new Fake(300);
    new Scrub([on] as unknown as Animation[]).release(true, () => {});
    expect(on.log).toEqual(['pause', 'play']);
  });
});
