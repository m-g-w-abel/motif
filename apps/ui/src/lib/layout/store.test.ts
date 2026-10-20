import { describe, expect, it } from 'vitest';
import { withName } from './store.svelte';

describe('withName', () => {
  it('replaces an existing name and keeps comments', () => {
    const text = '// mine\nlayout version=1 name=Standard\nview v { queue }\n';
    expect(withName(text, 'Standard (custom)')).toBe(
      '// mine\nlayout version=1 name="Standard (custom)"\nview v { queue }\n',
    );
  });

  it('adds a name when there is none', () => {
    expect(withName('layout version=1\n', 'New')).toBe('layout version=1 name=New\n');
  });
});
