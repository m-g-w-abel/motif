import { describe, expect, it } from 'vitest';
import { PlayerSheet, STEP, released, stepTo } from './sheet.svelte';

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

/** Just what the sheet uses of a transition, recording what it was asked. */
function fakeMotion() {
  const log: string[] = [];
  let built = 0;
  const make = () => {
    let end!: () => void;
    const t = {
      progress: 0,
      heading: null as 0 | 1 | null,
      ended: false,
      done: new Promise<void>((r) => (end = r)),
      seek(p: number) {
        if (t.heading !== null) return;
        t.progress = p;
        log.push(`seek ${p}`);
      },
      play(to: 0 | 1) {
        if (t.heading !== null) return t.done;
        t.heading = to;
        log.push(`play ${to}`);
        return t.done;
      },
      grab() {
        if (t.ended) return null;
        t.heading = null;
        log.push(`grab ${t.progress}`);
        return t.progress;
      },
      finish() {
        t.ended = true;
        t.progress = t.heading!;
        end();
      },
    };
    return t;
  };
  let last: ReturnType<typeof make> | undefined;
  const motion = {
    toggle: () => ((built++, (last = make())) as unknown as import('./motion/transition').Transition),
    step: (apply: () => void) => {
      log.push('step');
      apply();
    },
  };
  return { motion, log, built: () => built, last: () => last! };
}

describe('PlayerSheet open and close', () => {
  const settle = () => new Promise((r) => setTimeout(r));

  it('plays one transition open from the bar, and the same kind closed from the sheet', async () => {
    const sheet = new PlayerSheet();
    const m = fakeMotion();
    sheet.motion = m.motion;
    sheet.expand();
    expect(m.log).toEqual(['seek 0', 'play 1']);
    expect(sheet.open).toBe(true);
    expect(sheet.moving).toBe(true);
    expect(sheet.rest).toBe(false);
    m.last().finish();
    await settle();
    expect(sheet.moving).toBe(false);
    expect(sheet.rest).toBe(true);
    m.log.length = 0;
    sheet.collapse();
    expect(m.log).toEqual(['seek 1', 'play 0']);
    expect(sheet.open).toBe(false);
  });

  it('catches a closing sheet where it is drawn when opened again, instead of starting over', () => {
    const sheet = new PlayerSheet();
    const m = fakeMotion();
    sheet.motion = m.motion;
    sheet.open = true;
    sheet.collapse();
    m.last().progress = 0.4;
    sheet.expand();
    expect(m.built()).toBe(1);
    expect(m.log.slice(-2)).toEqual(['grab 0.4', 'play 1']);
    expect(sheet.open).toBe(true);
  });

  it('draws step changes only while open and still', async () => {
    const sheet = new PlayerSheet();
    const m = fakeMotion();
    sheet.motion = m.motion;
    sheet.expand();
    sheet.up();
    expect(sheet.step).toBe('half');
    expect(m.log).not.toContain('step');
    m.last().finish();
    await settle();
    sheet.up();
    expect(sheet.step).toBe('full');
    expect(m.log.at(-1)).toBe('step');
  });

  it('goes back to now playing once closed from the queue, not before (it is still drawn)', async () => {
    const sheet = new PlayerSheet();
    const m = fakeMotion();
    sheet.motion = m.motion;
    sheet.open = true;
    sheet.step = 'full';
    sheet.collapse();
    expect(sheet.step).toBe('full');
    m.last().finish();
    await settle();
    expect(sheet.step).toBe('now');
  });
});
