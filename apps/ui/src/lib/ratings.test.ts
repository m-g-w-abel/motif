import { describe, expect, it } from 'vitest';
import { nextRating, ratingLabel } from './ratings';

describe('nextRating', () => {
  it('picks a rating, and picking the same one again clears it', () => {
    expect(nextRating(null, 7)).toBe(7);
    expect(nextRating(3, 7)).toBe(7);
    expect(nextRating(7, 7)).toBeNull();
  });
});

describe('ratingLabel', () => {
  it('names half and whole stars', () => {
    expect(ratingLabel(1)).toBe('0.5 stars');
    expect(ratingLabel(2)).toBe('1 star');
    expect(ratingLabel(9)).toBe('4.5 stars');
    expect(ratingLabel(10)).toBe('5 stars');
  });
});
