import { describe, expect, it } from 'vitest';
import { nextRating, starsLabel, starsOf } from './ratings';

describe('starsOf', () => {
  it('rounds half stars up and shows nothing for no rating', () => {
    expect(starsOf(null)).toBe(0);
    expect(starsOf(undefined)).toBe(0);
    expect(starsOf(1)).toBe(1);
    expect(starsOf(2)).toBe(1);
    expect(starsOf(7)).toBe(4);
    expect(starsOf(10)).toBe(5);
  });
});

describe('nextRating', () => {
  it('stores whole stars as even half-star values', () => {
    expect(nextRating(null, 3)).toBe(6);
    expect(nextRating(2, 5)).toBe(10);
  });

  it('clears when the star chosen is what the rating already shows', () => {
    expect(nextRating(6, 3)).toBeNull();
    expect(nextRating(5, 3)).toBeNull(); // 2.5 shows as three
  });
});

describe('starsLabel', () => {
  it('names one star and many', () => {
    expect(starsLabel(1)).toBe('1 star');
    expect(starsLabel(4)).toBe('4 stars');
  });
});
