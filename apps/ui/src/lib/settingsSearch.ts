import type { IconName } from './components/Icon.svelte';
import { SETTINGS_GROUPS, type SettingsGroup } from './settingsView.svelte';

export const GROUP_INFO: Record<SettingsGroup, { label: string; summary: string; icon: IconName; colour: string }> = {
  interface: { label: 'Interface', summary: 'Theme, scale, font, colours', icon: 'columns', colour: '#e0629a' },
  library: { label: 'Library', summary: 'Music folders, scanning', icon: 'library', colour: '#4aa3df' },
  playback: { label: 'Playback', summary: 'Output, ReplayGain, decoders', icon: 'music', colour: '#e8a03c' },
  keyboard: { label: 'Keyboard', summary: 'Shortcuts, vim keys', icon: 'list', colour: '#8d8d99' },
  gestures: { label: 'Gestures', summary: 'Swipes and holds on touch screens', icon: 'grid', colour: '#7bc96f' },
  integrations: { label: 'Integrations', summary: 'Lyrics, scrobbling, Discord', icon: 'lyrics', colour: '#a07be0' },
  devices: { label: 'Devices', summary: 'Engines, pairing, LAN, MPD', icon: 'split', colour: '#3fc1b0' },
  technical: { label: 'Technical', summary: 'Engine connection', icon: 'settings', colour: '#d9695f' },
  about: { label: 'About', summary: 'What Motif is', icon: 'info', colour: '#6f8fe0' },
};

/** Every setting by name, for the phone's search. Keep in step with the labels in `SettingsScreen.svelte`. */
const SETTINGS: { group: SettingsGroup; label: string }[] = [
  { group: 'interface', label: 'Theme' },
  { group: 'interface', label: 'Scale' },
  { group: 'interface', label: 'Font' },
  { group: 'interface', label: 'Window buttons in the app' },
  { group: 'interface', label: 'Colours from artwork' },
  { group: 'interface', label: 'Accent' },
  { group: 'library', label: 'All files access' },
  { group: 'library', label: 'Music folders' },
  { group: 'playback', label: 'Output' },
  { group: 'playback', label: 'ReplayGain' },
  { group: 'playback', label: 'Decoders' },
  { group: 'keyboard', label: 'Vim keys (navigation only)' },
  { group: 'integrations', label: 'Lyrics from LRCLIB' },
  { group: 'integrations', label: 'ListenBrainz' },
  { group: 'integrations', label: 'Last.fm' },
  { group: 'integrations', label: 'Libre.fm' },
  { group: 'integrations', label: 'Discord status' },
  { group: 'devices', label: 'Engine in control' },
  { group: 'devices', label: 'Other engines' },
  { group: 'devices', label: 'Remote playback controls' },
  { group: 'devices', label: 'Other devices' },
  { group: 'devices', label: 'Paired devices' },
  { group: 'devices', label: 'Approve new devices automatically' },
  { group: 'devices', label: 'MPD clients' },
  { group: 'devices', label: 'Pairing log' },
  { group: 'technical', label: 'Engine connection' },
];

/** Groups the phone's list shows: Keyboard needs a keyboard. */
export const phoneGroups = (): SettingsGroup[] => SETTINGS_GROUPS.filter((g) => g !== 'keyboard');

/** Settings (and groups) whose name has every word of `query`; a group comes before its settings. */
export function searchSettings(query: string, phone = false): { group: SettingsGroup; label: string }[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const shown = phone ? phoneGroups() : [...SETTINGS_GROUPS];
  const entries = shown.flatMap((group) => [
    { group, label: GROUP_INFO[group].label },
    ...SETTINGS.filter((s) => s.group === group),
  ]);
  return entries.filter(({ label }) => words.every((w) => label.toLowerCase().includes(w)));
}
