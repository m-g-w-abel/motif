import { engines, type EngineClient } from './engine.svelte';
import { android, type AndroidBridge } from './android';
import { LOCAL_ID } from './engines';

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

function pickAndroidFolder(android: AndroidBridge): Promise<string | null> {
  return new Promise((resolve) => {
    (window as { motifFolderPicked?: (uri: string | null) => void }).motifFolderPicked = resolve;
    android.pickFolder();
  });
}

/**
 * Asks for a folder on the machine running the engine: the system folder picker
 * on Android, a native folder picker in the desktop app, a path typed in a
 * browser (which can't browse that machine). Null if none was picked.
 */
async function pickFolder(engine: EngineClient, title: string): Promise<string | null> {
  let path: string | null;
  const storage = androidStorage();
  if (storage) {
    if (!storage.hasAllFilesAccess()) {
      engine.error = 'Motif needs All files access to read your music. Allow it, then pick the folder again.';
      storage.openAllFilesSettings();
      return null;
    }
    const uri = await pickAndroidFolder(storage);
    if (!uri) return null;
    path = treeUriToPath(uri);
    if (!path) {
      engine.error = 'Pick a folder on the phone’s storage or an SD card; Motif can’t read that location by path.';
      return null;
    }
  } else if ('__TAURI_INTERNALS__' in window && !android) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false, title });
    path = typeof picked === 'string' ? picked : null;
  } else {
    path = window.prompt(`${title} (a path on the computer running Motif):`);
  }
  return path?.trim() || null;
}

/** Asks for a music folder and adds it to the library. */
export async function addFolder(engine: EngineClient): Promise<void> {
  const path = await pickFolder(engine, 'Add a music folder');
  if (path) await engine.query({ type: 'add_root', path }).catch((e: Error) => (engine.error = e.message));
}

/** Asks where downloads go (a library folder; the engine moves nothing already there). */
export async function setDownloadFolder(engine: EngineClient): Promise<void> {
  const path = await pickFolder(engine, 'Save downloads in');
  if (path) await engine.query({ type: 'set_download_folder', path }).catch((e: Error) => (engine.error = e.message));
}
