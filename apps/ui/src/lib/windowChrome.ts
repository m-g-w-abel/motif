/** Own window decorations only make sense in the desktop app, never on Android. */
export const canDecorate = '__TAURI_INTERNALS__' in window && !/Android/i.test(navigator.userAgent);
const mac = /Mac/i.test(navigator.userAgent);
/** macOS keeps its own traffic lights, so only the other systems get buttons. */
export const drawsButtons = canDecorate && !mac;

/** Applies the setting to the window; changes take effect at once, no restart. */
export async function applyDecorations(own: boolean) {
  if (!canDecorate) return;
  const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
  // macOS: an overlay title bar leaves the traffic lights where they are (untested).
  if (mac) await win.setTitleBarStyle(own ? 'overlay' : 'visible');
  else await win.setDecorations(!own);
}

/** Linux's undecorated windows get no resize edges from the webview, so the UI draws its own. */
export const needsResizeEdges = canDecorate && /Linux/i.test(navigator.userAgent);

export async function startResize(direction: string) {
  const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
  await win.startResizeDragging(direction as 'North');
}

export async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
  await win[action]();
}
