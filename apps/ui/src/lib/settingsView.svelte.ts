export const SETTINGS_GROUPS = ['interface', 'library', 'playback', 'keyboard', 'integrations', 'devices', 'technical', 'about'] as const;
export type SettingsGroup = (typeof SETTINGS_GROUPS)[number];

/** Which Settings group is showing in the music area; null when Settings is closed. */
export class SettingsView {
  group = $state<SettingsGroup | null>(null);
  /** The Pair a device dialog is open (it shows waiting devices itself, so the app's prompt hides). */
  pairing = $state(false);
  /** The Add an engine dialog is open. */
  addingEngine = $state(false);

  open(group: SettingsGroup = 'interface') {
    this.group = group;
  }

  close() {
    this.group = null;
  }
}
