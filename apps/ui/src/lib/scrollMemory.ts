/**
 * Where each list was left scrolled, keyed by source, layout and place in it.
 * Kept in memory only: it lasts for this session, and no further.
 */
export const scrollMemory = new Map<string, number>();
