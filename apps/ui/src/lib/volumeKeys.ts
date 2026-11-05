import type { EngineClient } from './engine.svelte';
import { toasts } from './toast.svelte';

/** Android's media volume has 15 steps; the keys move a remote engine by the same amount. */
const STEPS = 15;
/** Presses this close together step from the last one, not from an engine that hasn't answered yet. */
const SETTLE_MS = 1000;

/** The volume one step up (1) or down (-1) from `volume`, on a step. */
export function stepVolume(volume: number, direction: 1 | -1): number {
  // The small slack keeps a volume already on a step from counting as between two.
  const from = direction > 0 ? Math.floor(volume * STEPS + 1e-6) : Math.ceil(volume * STEPS - 1e-6);
  return Math.min(STEPS, Math.max(0, from + direction)) / STEPS;
}

/**
 * A key press handler: sends absolute volumes (safe to retry), stepping from
 * `read()` or the last press. Drops presses while `read()` is null (volume not known yet).
 */
export function volumeKeys(read: () => number | null, send: (volume: number) => void, now = Date.now) {
  let last: { volume: number; at: number } | null = null;
  return (direction: 1 | -1) => {
    const base = last && now() - last.at < SETTLE_MS ? last.volume : read();
    if (base === null) return;
    last = { volume: stepVolume(base, direction), at: now() };
    send(last.volume);
  };
}

const android = (globalThis as { MotifAndroid?: { setVolumeKeys(on: boolean): void } }).MotifAndroid;
const hook = globalThis as { motifVolumeKey?: (direction: 1 | -1) => void };
// A reload keeps the activity: without this, keys stay caught with nothing to catch them.
android?.setVolumeKeys(false);

/**
 * While a remote engine is current, the phone's volume keys (`MainActivity.kt`)
 * set its volume instead of the phone's. Returns the undo.
 */
export function routeVolumeKeys(engine: EngineClient): () => void {
  if (!android) return () => {};
  let toast: number | null = null;
  hook.motifVolumeKey = volumeKeys(
    () => engine.player?.volume ?? null,
    (volume) => {
      engine.run({ type: 'set_volume', volume });
      // The volume slider is hidden on a phone; one toast, replaced on each press.
      if (toast !== null) toasts.dismiss(toast);
      toast = toasts.show(`Volume ${Math.round(volume * 100)}%`);
    },
  );
  android.setVolumeKeys(true);
  return () => {
    android.setVolumeKeys(false);
    delete hook.motifVolumeKey;
  };
}
