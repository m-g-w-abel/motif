import { describe, expect, it } from 'vitest';
import { markActions, nextRating, starsLabel, starsOf } from './ratings';

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

describe('markActions', () => {
  const loved = { favourite: true, rating: null };
  const plain = { favourite: false, rating: null };
  const rated = { favourite: false, rating: 6 };

  it('offers Love for songs not loved, Unlove for songs loved, both for a mix', () => {
    expect(markActions('hearts', [plain, plain])).toEqual(['love']);
    expect(markActions('hearts', [loved])).toEqual(['unlove']);
    expect(markActions('hearts', [loved, plain])).toEqual(['love', 'unlove']);
    expect(markActions('hearts', [undefined])).toEqual(['love']); // not looked up yet
  });

  it('offers Clear rating only when a song is rated, and never hearts in stars mode', () => {
    expect(markActions('stars', [plain, loved])).toEqual([]);
    expect(markActions('stars', [plain, rated])).toEqual(['clear']);
  });

  it('offers nothing for no songs', () => {
    expect(markActions('hearts', [])).toEqual([]);
  });
});
