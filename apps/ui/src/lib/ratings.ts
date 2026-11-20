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

export type MarkAction = 'love' | 'unlove' | 'clear';

/**
 * What a set of songs' menu offers for the Hearts or Stars setting (`marks`: what each shows, `undefined` while
 * not known): Love if any isn't loved, Unlove if any is, so a mix shows both; Clear rating if any is rated.
 */
export function markActions(style: RatingStyle, marks: ({ favourite: boolean; rating: number | null } | null | undefined)[]): MarkAction[] {
  if (!marks.length) return [];
  if (style === 'stars') return marks.some((m) => m?.rating != null) ? ['clear'] : [];
  const actions: MarkAction[] = [];
  if (marks.some((m) => !m?.favourite)) actions.push('love');
  if (marks.some((m) => m?.favourite)) actions.push('unlove');
  return actions;
}
