import { onBackButtonPress } from '@tauri-apps/api/app';
import type { Slide } from './gestures';

/** Something system back closes; listed topmost first. */
export interface Layer {
  open: boolean;
  close(): void;
  /** Shows a system back swipe under way (predictive back): what moves with it, if anything. */
  follow?: () => Slide | undefined;
}

/** Closes the first open layer. False when none is open. */
export function closeTop(layers: Layer[]): boolean {
  const top = layers.find((layer) => layer.open);
  top?.close();
  return !!top;
}

/**
 * Android's predictive back swipe as it goes (`MainActivity.kt` reports its
 * progress, 0 to 1): the top open layer follows it across `width()`, and
 * springs back if the swipe is cancelled. Letting go is an ordinary back
 * (`routeBack`), which finishes the move from there (`screens.ts` `leave`).
 */
export function backSwipe(layers: () => Layer[], width: () => number) {
  let shown: Slide | undefined;
  return {
    start() {
      shown = layers().find((layer) => layer.open)?.follow?.();
    },
    progress(p: number) {
      shown?.move(p * width());
    },
    cancel() {
      shown?.end(null);
      shown = undefined;
    },
  };
}

const android = (globalThis as { MotifAndroid?: { moveToBack(): void; followBack?(): void } }).MotifAndroid;

/**
 * Android's back (gesture or button) closes the topmost layer from `layers()`;
 * with none open it leaves the app as Android does (`MainActivity.kt`),
 * keeping the page and playback. Returns the undo.
 */
export function routeBack(layers: () => Layer[]): () => void {
  if (!android) return () => {};
  const listener = onBackButtonPress(() => closeTop(layers()) || android.moveToBack());
  const own = globalThis as { motifBack?: ReturnType<typeof backSwipe> };
  own.motifBack = backSwipe(layers, () => innerWidth);
  android.followBack?.();
  return () => {
    delete own.motifBack;
    void listener.then((l) => l.unregister());
  };
}
