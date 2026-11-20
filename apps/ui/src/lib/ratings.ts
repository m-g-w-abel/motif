/** Ratings count half stars: 1 (half a star) to 10 (five). */
export const MAX_HALVES = 10;

/** The rating after choosing `picked` on a track rated `current`: choosing the same again clears it. */
export function nextRating(current: number | null, picked: number): number | null {
  return picked === current ? null : picked;
}

/** "4.5 stars", for tooltips and screen readers. */
export function ratingLabel(halves: number): string {
  const stars = halves / 2;
  return `${stars} ${stars === 1 ? 'star' : 'stars'}`;
}
