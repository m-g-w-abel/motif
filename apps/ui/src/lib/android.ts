/** What `MainActivity.Storage` (`MainActivity.kt`) adds to the Android app's WebView: keep the two in step. */
export interface AndroidBridge {
  /** All files access (Android 11+) or storage read permission (older). */
  hasAllFilesAccess(): boolean;
  /** Opens the system toggle (or asks, before Android 11). */
  openAllFilesSettings(): void;
  /** Opens the system folder picker; the tree URI (or null) arrives at `window.motifFolderPicked`. */
  pickFolder(): void;
  /** Dark status and navigation bar icons (for a light theme), or light ones. */
  setBarIconsDark(dark: boolean): void;
  /** Holds Wi-Fi's multicast lock (for mDNS) while on. */
  setMulticast(on: boolean): void;
  /** The motif:// link the app was opened with, once. */
  takeLink(): string | null;
  /** Asks the launcher to pin a shortcut; false if it can't pin any. */
  pinShortcut(link: string, label: string, cover: string | null): boolean;
  /** Reports the system back swipe to `window.motifBack`. */
  followBack(): void;
  /** Android's font size setting (1 is the default). */
  fontScale(): number;
  /** Sends the volume keys to `window.motifVolumeKey` while on. */
  setVolumeKeys(on: boolean): void;
  /** A remote engine's playback (`RemoteMedia` JSON) for the notification, or null for the phone's. */
  setRemoteMedia(json: string | null): void;
  /** Puts the app behind, as back on the root screen does. */
  moveToBack(): void;
}

/** The Android app's bridge; undefined anywhere else. */
export const android = (globalThis as { MotifAndroid?: AndroidBridge }).MotifAndroid;
