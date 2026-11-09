import { MediaQuery } from 'svelte/reactivity';
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

  /** On a phone, Settings opens on the list of groups; `group` is the page behind it. */
  list = $state(false);
  #phone: () => boolean;

  /** `phone` says whether the screen is phone width; tests pass their own (there is no window). */
  constructor(phone?: () => boolean) {
    const narrow = typeof window === 'undefined' ? { current: false } : new MediaQuery('(max-width: 700px)');
    this.#phone = phone ?? (() => narrow.current);
  }

  /** Phone width: Settings is a list of groups, each opening its own page. */
  get phone() {
    return this.#phone();
  }

  /** Opens `group`, or with none the phone's list (the desktop's first group). */
  open(group?: SettingsGroup) {
    this.list = group === undefined && this.phone;
    this.group = group ?? 'interface';
  }

  /** From a group's page to the list; from the list (or the desktop) closes Settings. */
  back() {
    if (this.phone && !this.list && this.group) this.list = true;
    else this.close();
  }

  /** Pairs with `link` as if its QR was scanned. */
  pairWith(link: PairLink) {
    this.open('devices');
    this.pairLink = link;
    this.addingEngine = true;
  }

  close() {
    this.group = null;
    this.list = false;
  }
}
