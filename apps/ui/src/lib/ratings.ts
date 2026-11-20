/** Which of a track's two marks the app shows and sets: a rating in stars, or a heart. Both stay stored. */
export type RatingStyle = 'stars' | 'hearts';

/** Ratings are stored in half stars (1 to 10), where the app sets whole stars only. */
export const STARS = 5;

/** The whole stars a stored rating shows: half stars round up. */
export function starsOf(halves: number | null | undefined): number {
  return halves ? Math.ceil(halves / 2) : 0;
}

/** The rating to store after choosing `stars` on a track rated `current`: choosing what it shows again clears it. */
export function nextRating(current: number | null, stars: number): number | null {
  return starsOf(current) === stars ? null : stars * 2;
}

/** "3 stars", for tooltips and screen readers. */
export function starsLabel(stars: number): string {
  return `${stars} ${stars === 1 ? 'star' : 'stars'}`;
}
