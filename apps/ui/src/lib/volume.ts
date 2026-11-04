import type { EngineClient } from './engine.svelte';

/**
 * Sets the volume from a drag: a drag fires many moves per frame, so it
 * sends at most one command per frame, always ending on the latest value.
 */
export function volumeSetter(engine: EngineClient) {
  let pending: number | null = null;
  return (v: number) => {
    if (pending === null) {
      requestAnimationFrame(() => {
        engine.run({ type: 'set_volume', volume: pending ?? v });
        pending = null;
      });
    }
    pending = Math.round(v * 100) / 100;
  };
}
