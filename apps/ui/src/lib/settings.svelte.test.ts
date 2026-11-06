import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mapped } from './gestures';
import { SettingsStore } from './settings.svelte';

const storage = new Map<string, string>();
vi.stubGlobal('localStorage', {
  getItem: (k: string) => storage.get(k) ?? null,
  setItem: (k: string, v: string) => storage.set(k, v),
  removeItem: (k: string) => storage.delete(k),
});

beforeEach(() => storage.clear());

describe('gesture settings', () => {
  it('start with no overrides, also for settings saved before gestures were remappable', () => {
    storage.set('settings', JSON.stringify({ theme: 'paper' }));
    expect(new SettingsStore().current.gestures).toEqual({});
  });

  it('survive a restart', () => {
    new SettingsStore().set('gestures', { artwork: { 'drag-vertical': 'seek' }, row: { 'long-press': 'none' } });
    const after = new SettingsStore().current.gestures;
    expect(mapped(after, 'artwork')['drag-vertical']).toBe('seek');
    expect(mapped(after, 'row')['long-press']).toBeUndefined();
  });

  it('remap one gesture at a time and reset to the defaults', () => {
    const s = new SettingsStore();
    s.remap('bar', 'swipe-left', 'previous');
    s.remap('bar', 'swipe-right', 'none');
    expect(mapped(s.current.gestures, 'bar')).toEqual({ 'swipe-left': 'previous' });
    s.remap('artwork', 'drag-vertical', 'seek');
    expect(mapped(s.current.gestures, 'bar')).toEqual({ 'swipe-left': 'previous' });
    s.set('gestures', {});
    expect(mapped(new SettingsStore().current.gestures, 'bar')).toEqual({ 'swipe-left': 'next', 'swipe-right': 'previous' });
  });
});
