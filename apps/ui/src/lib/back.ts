import { onBackButtonPress } from '@tauri-apps/api/app';

/** Something system back closes; listed topmost first. */
export interface Layer {
  open: boolean;
  close(): void;
}

/** Closes the first open layer. False when none is open. */
export function closeTop(layers: Layer[]): boolean {
  const top = layers.find((layer) => layer.open);
  top?.close();
  return !!top;
}

const android = (globalThis as { MotifAndroid?: { moveToBack(): void } }).MotifAndroid;

/**
 * Android's back (gesture or button) closes the topmost layer from `layers()`;
 * with none open it leaves the app as Android does (`MainActivity.kt`),
 * keeping the page and playback. Returns the undo.
 */
export function routeBack(layers: () => Layer[]): () => void {
  if (!android) return () => {};
  const listener = onBackButtonPress(() => closeTop(layers()) || android.moveToBack());
  return () => void listener.then((l) => l.unregister());
}
