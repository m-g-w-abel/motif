import { engines, type EngineClient } from './engine.svelte';
import { LOCAL_ID } from './engines';

/** What `MainActivity.kt` adds to the Android app's WebView. */
interface AndroidStorage {
  /** All files access (Android 11+) or storage read permission (older). */
  hasAllFilesAccess(): boolean;
  /** Opens the system toggle (or asks, before Android 11). */
  openAllFilesSettings(): void;
  /** Opens the system folder picker; the tree URI (or null) arrives at `window.motifFolderPicked`. */
  pickFolder(): void;
  /** The system bar insets in CSS pixels: top, right, bottom, left. Changes arrive at `window.motifInsets`. */
  insets(): string;
  /** Dark status and navigation bar icons (for a light theme), or light ones. */
  setBarIconsDark(dark: boolean): void;
}

const android = (globalThis as { MotifAndroid?: AndroidStorage }).MotifAndroid;

/** Keeps `--inset-*` at the Android system bars' size, and the bar icons readable on the theme. */
export function trackSystemBars(): void {
  if (!android) return;
  const apply = () => {
    const [top, right, bottom, left] = android.insets().split(' ');
    const style = document.documentElement.style;
    style.setProperty('--inset-top', `${top}px`);
    style.setProperty('--inset-right', `${right}px`);
    style.setProperty('--inset-bottom', `${bottom}px`);
    style.setProperty('--inset-left', `${left}px`);
  };
  (globalThis as { motifInsets?: () => void }).motifInsets = apply;
  apply();
}

export const setBarIconsDark = (dark: boolean) => android?.setBarIconsDark(dark);

/** The Android app's storage bridge, while this phone's own engine is in control (a remote engine's folders are typed, as in a browser). */
export const androidStorage = () => (engines.current.id === LOCAL_ID ? android : undefined);


/**
 * The plain path of a folder picked in Android's folder picker, or null when
 * it isn't on the phone's storage, an SD card or a USB drive (cloud providers,
 * the Downloads shortcut).
 */
export function treeUriToPath(uri: string): string | null {
  const match = /^content:\/\/com\.android\.externalstorage\.documents\/tree\/([^/]+)/.exec(uri);
  if (!match) return null;
  const id = decodeURIComponent(match[1]);
  const colon = id.indexOf(':');
  if (colon < 0) return null;
  const volume = id.slice(0, colon);
  const rest = id.slice(colon + 1);
  if (!/^[\w-]+$/.test(volume) || rest.split('/').includes('..')) return null;
  const root = volume === 'primary' ? '/storage/emulated/0' : `/storage/${volume}`;
  return rest ? `${root}/${rest}` : root;
}

function pickAndroidFolder(android: AndroidStorage): Promise<string | null> {
  return new Promise((resolve) => {
    (window as { motifFolderPicked?: (uri: string | null) => void }).motifFolderPicked = resolve;
    android.pickFolder();
  });
}

/**
 * Asks for a music folder and adds it to the library: the system folder picker
 * on Android, a native folder picker in the desktop app, a path typed in a
 * browser (the folder is on the machine running the engine, which a browser
 * can't browse).
 */
export async function addFolder(engine: EngineClient): Promise<void> {
  let path: string | null;
  const storage = androidStorage();
  if (storage) {
    if (!storage.hasAllFilesAccess()) {
      engine.error = 'Motif needs All files access to read your music. Allow it, then add the folder again.';
      storage.openAllFilesSettings();
      return;
    }
    const uri = await pickAndroidFolder(storage);
    if (!uri) return;
    path = treeUriToPath(uri);
    if (!path) {
      engine.error = 'Pick a folder on the phone’s storage or an SD card; Motif can’t read that location by path.';
      return;
    }
  } else if ('__TAURI_INTERNALS__' in window && !android) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false, title: 'Add a music folder' });
    path = typeof picked === 'string' ? picked : null;
  } else {
    path = window.prompt('Music folder to add (a path on the computer running Motif):');
  }
  if (!path?.trim()) return;
  await engine.query({ type: 'add_root', path: path.trim() }).catch((e: Error) => (engine.error = e.message));
}
