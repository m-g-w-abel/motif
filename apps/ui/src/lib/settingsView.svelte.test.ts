import { describe, expect, it } from 'vitest';
import { SettingsView } from './settingsView.svelte';

describe('SettingsView on a phone', () => {
  it('opens on the list, then a page, and back returns step by step', () => {
    const view = new SettingsView(() => true);
    view.open();
    expect(view.list).toBe(true);
    view.open('devices');
    expect([view.list, view.group]).toEqual([false, 'devices']);
    view.back();
    expect([view.list, view.group]).toEqual([true, 'devices']);
    view.back();
    expect(view.group).toBeNull();
    expect(view.list).toBe(false);
  });

  it('opening the list twice changes nothing', () => {
    const view = new SettingsView(() => true);
    view.open();
    view.open();
    expect(view.list).toBe(true);
  });
});

describe('SettingsView on the desktop', () => {
  it('never shows the list, and back closes', () => {
    const view = new SettingsView(() => false);
    view.open();
    expect([view.list, view.group]).toEqual([false, 'interface']);
    view.back();
    expect(view.group).toBeNull();
  });
});
