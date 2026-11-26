/** A seek sent to the engine: where to (`ms`), and our clock when it was sent (`at`). */
export interface SentSeek {
  ms: number;
  at: number;
}

/** How long the seek bar waits for a seek to land: a streamed one can take a dial and a stall (#282). */
export const HOLD_MS = 10_000;
/** How near the target the engine's position counts as having landed. */
const NEAR_MS = 1_500;

/**
 * The seek the bar still shows in place of the engine's position (`reported`), or null once it
 * has landed or given up. Without it the bar snaps back to the old position until the engine
 * reports the new one, which on a streamed song is the whole seek (#279).
 */
export function settle(sent: SentSeek | null, reported: number, now: number): SentSeek | null {
  if (!sent || now - sent.at > HOLD_MS || Math.abs(reported - sent.ms) <= NEAR_MS) return null;
  return sent;
}
