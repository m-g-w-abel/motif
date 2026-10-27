import { describe, expect, it } from 'vitest';
import { SHORTCUTS, shortcutFor } from './shortcuts';

const press = (key: string, mods: Partial<Parameters<typeof shortcutFor>[0]> = {}) =>
  shortcutFor({ key, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...mods });

describe('shortcutFor', () => {
  it('finds bound keys', () => {
    expect(press(' ')).toBe('play_pause');
    expect(press('/')).toBe('search');
    expect(press('f', { ctrlKey: true })).toBe('search');
    expect(press(',', { metaKey: true })).toBe('settings');
  });

  it('tells Shift+arrow from the plain arrow', () => {
    expect(press('ArrowLeft')).toBe('seek_back');
    expect(press('ArrowLeft', { shiftKey: true })).toBe('previous');
    expect(press('ArrowRight', { shiftKey: true })).toBe('next');
  });

  it('accepts ? with or without Shift', () => {
    expect(press('?')).toBe('help');
    expect(press('?', { shiftKey: true })).toBe('help');
  });

  it('ignores other keys and extra modifiers', () => {
    expect(press('f')).toBeNull();
    expect(press(' ', { ctrlKey: true })).toBeNull();
    expect(press('ArrowLeft', { altKey: true })).toBeNull();
  });

  it('has unique ids and no two shortcuts on one key', () => {
    expect(new Set(SHORTCUTS.map((s) => s.id)).size).toBe(SHORTCUTS.length);
    const bound = SHORTCUTS.flatMap((s) => s.bindings ?? []).map((b) => JSON.stringify([b.key, !!b.ctrl, !!b.alt, b.shift]));
    expect(new Set(bound).size).toBe(bound.length);
  });
});
