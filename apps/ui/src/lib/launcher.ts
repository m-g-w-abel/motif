import { android } from './android';
import type { MenuItem } from './contextMenu.svelte';
import { toasts } from './toast.svelte';
import { viewLink, viewTitle, type View } from './views.svelte';


/** Asks the launcher to pin a home-screen shortcut to `view`, with the artwork at `cover` as its icon (else the app's). */
function addToLauncher(view: View, cover: string | null) {
  if (!android?.pinShortcut(viewLink(view), viewTitle(view), cover)) toasts.show("This launcher can't add shortcuts.");
}

/** "Add to launcher" for `view`'s ⋮ menu: none off Android. */
export function launcherItems(view: View, cover: string | null = null): MenuItem[] {
  return android ? [{ label: 'Add to launcher', run: () => addToLauncher(view, cover) }] : [];
}
