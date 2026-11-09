import { describe, expect, it } from 'vitest';
import { SETTINGS_GROUPS } from './settingsView.svelte';
import { GROUP_INFO, phoneGroups, searchSettings } from './settingsSearch';

describe('searchSettings', () => {
  it('finds a setting by part of its name, ignoring case', () => {
    expect(searchSettings('replaygain')).toEqual([{ group: 'playback', label: 'ReplayGain' }]);
  });

  it('matches every word, in any order', () => {
    expect(searchSettings('keys vim')).toEqual([{ group: 'keyboard', label: 'Vim keys (navigation only)' }]);
  });

  it('matches a group name too, listing that group first', () => {
    expect(searchSettings('devices')[0]).toEqual({ group: 'devices', label: 'Devices' });
  });

  it('finds nothing for gibberish or a blank query', () => {
    expect(searchSettings('zzzz')).toEqual([]);
    expect(searchSettings('  ')).toEqual([]);
  });
});

describe('phoneGroups', () => {
  it('hides Keyboard but keeps the others in order', () => {
    expect(phoneGroups()).not.toContain('keyboard');
    expect(phoneGroups()).toEqual(SETTINGS_GROUPS.filter((g) => g !== 'keyboard'));
  });

  it('keeps Keyboard settings out of search results', () => {
    expect(searchSettings('vim', true)).toEqual([]);
  });
});

it('describes every group', () => {
  for (const g of SETTINGS_GROUPS) expect(GROUP_INFO[g].summary).not.toBe('');
});
