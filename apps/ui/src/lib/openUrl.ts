/** Where Help goes: the project's docs, starting from the README. */
export const HELP_URL = 'https://github.com/m-g-w-abel/motif#readme';

/** Opens a web page in the system browser: Tauri's opener in the desktop app, a new tab in a browser. */
export async function openUrl(url: string): Promise<void> {
  if ('__TAURI_INTERNALS__' in window) {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  } else {
    window.open(url, '_blank', 'noopener');
  }
}
