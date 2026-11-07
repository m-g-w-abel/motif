import type { PairLink } from './discover';

export const SETTINGS_GROUPS = ['interface', 'library', 'playback', 'keyboard', 'gestures', 'integrations', 'devices', 'technical', 'about'] as const;
export type SettingsGroup = (typeof SETTINGS_GROUPS)[number];

/** Which Settings group is showing in the music area; null when Settings is closed. */
export class SettingsView {
  group = $state<SettingsGroup | null>(null);
  /** The Pair a device dialog is open (it shows waiting devices itself, so the app's prompt hides). */
  pairing = $state(false);
  /** The Add an engine dialog is open. */
  addingEngine = $state(false);
  /** A pairing link the app was opened with (`MainActivity.kt`), for the Add an engine dialog to pair with. */
  pairLink = $state<PairLink | null>(null);
  /** Its camera is scanning a QR; system back closes the camera, not the dialog. */
  scanning = $state(false);

  open(group: SettingsGroup = 'interface') {
    this.group = group;
  }

  /** Pairs with `link` as if its QR was scanned. */
  pairWith(link: PairLink) {
    this.open('devices');
    this.pairLink = link;
    this.addingEngine = true;
  }

  close() {
    this.group = null;
  }
}
