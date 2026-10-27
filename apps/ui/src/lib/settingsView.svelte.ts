export const SETTINGS_GROUPS = ['interface', 'library', 'playback', 'keyboard', 'technical', 'about'] as const;
export type SettingsGroup = (typeof SETTINGS_GROUPS)[number];

/** Which Settings group is showing in the music area; null when Settings is closed. */
export class SettingsView {
  group = $state<SettingsGroup | null>(null);

  open(group: SettingsGroup = 'interface') {
    this.group = group;
  }

  close() {
    this.group = null;
  }
}
