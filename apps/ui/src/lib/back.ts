import { onBackButtonPress } from '@tauri-apps/api/app';
import { android } from './android';
import type { Drive } from './motion/transition';

/** Something system back closes; listed topmost first. */
export interface Layer {
  open: boolean;
  close(): void;
  /** Shows a system back swipe under way (predictive back): what it drives, if anything. */
  follow?: () => Drive | undefined;
}

/** Closes the first open layer. False when none is open. */
export function closeTop(layers: Layer[]): boolean {
  const top = layers.find((layer) => layer.open);
  top?.close();
  return !!top;
}

/**
 * Android's predictive back swipe as it goes (`MainActivity.kt` reports its
 * progress, 0 to 1) drives the top open layer's change, which springs back
 * if the swipe is cancelled. Letting go is an ordinary back (`routeBack`),
 * which finishes the change from there.
 */
export function backSwipe(layers: () => Layer[]) {
  let shown: Drive | undefined;
  return {
    start() {
      shown = layers().find((layer) => layer.open)?.follow?.();
    },
    progress(p: number) {
      shown?.seek(p);
    },
    cancel() {
      shown?.release(false);
      shown = undefined;
    },
  };
}


/**
 * Android's back (gesture or button) closes the topmost layer from `layers()`;
 * with none open it leaves the app as Android does (`MainActivity.kt`),
 * keeping the page and playback. Returns the undo.
 */
export function routeBack(layers: () => Layer[]): () => void {
  if (!android) return () => {};
  const listener = onBackButtonPress(() => closeTop(layers()) || android?.moveToBack());
  const own = globalThis as { motifBack?: ReturnType<typeof backSwipe> };
  own.motifBack = backSwipe(layers);
  android.followBack();
  return () => {
    delete own.motifBack;
    void listener.then((l) => l.unregister());
  };
}
