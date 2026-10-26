import type { EngineClient } from './engine.svelte';

/**
 * Asks for a music folder and adds it to the library: a native folder picker
 * in the desktop app, a path typed in a browser (the folder is on the machine
 * running the engine, which a browser can't browse).
 */
export async function addFolder(engine: EngineClient): Promise<void> {
  let path: string | null;
  if ('__TAURI_INTERNALS__' in window) {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false, title: 'Add a music folder' });
    path = typeof picked === 'string' ? picked : null;
  } else {
    path = window.prompt('Music folder to add (a path on the computer running Motif):');
  }
  if (!path?.trim()) return;
  await engine.query({ type: 'add_root', path: path.trim() }).catch((e: Error) => (engine.error = e.message));
}
