import { isAndroid } from './platform';

/** Own window decorations only make sense in the desktop app, never on Android. */
export const canDecorate = '__TAURI_INTERNALS__' in window && !isAndroid;
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

/** Calls `on` with whether the window is maximised, now and after each resize; returns a stop function. */
export function watchMaximised(on: (maximised: boolean) => void): () => void {
  let stop = () => {};
  let stopped = false;
  void (async () => {
    const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
    const check = async () => on(await win.isMaximized());
    await check();
    const unlisten = await win.onResized(check);
    if (stopped) unlisten();
    else stop = unlisten;
  })();
  return () => {
    stopped = true;
    stop();
  };
}

export async function startResize(direction: string) {
  const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
  await win.startResizeDragging(direction as 'North');
}

export async function windowAction(action: 'minimize' | 'toggleMaximize' | 'close') {
  const win = (await import('@tauri-apps/api/window')).getCurrentWindow();
  await win[action]();
}
