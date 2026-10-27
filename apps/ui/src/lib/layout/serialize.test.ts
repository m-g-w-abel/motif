import { describe, expect, it } from 'vitest';
import { parseLayout } from './parse';
import { builtinLayouts } from './presets';
import { serializeLayout } from './serialize';

const parsed = (text: string) => {
  const result = parseLayout(text);
  if (!result.ok) throw new Error(result.errors.map((e) => e.message).join('; '));
  return result.layout;
};

describe('serializeLayout', () => {
  it.each(builtinLayouts.map((s) => [s.fileName, s.text]))('round-trips %s', (_, text) => {
    const layout = parsed(text);
    expect(parsed(serializeLayout(layout))).toEqual(layout);
  });

  it('round-trips every option, top-level stacks and a phone variant', () => {
    const layout = parsed(`layout version=1 name="Odd \\"one\\"" icon=grid
tracklist headers=#false max-width=900 size=200 { columns title duration; }
split column size="40%" { album-grid card-size=120 columns=4; queue collapsible=#true; }
track-info style=hero
when phone { split column { controls; } }
`);
    expect(layout.root.kind).toBe('split');
    expect(parsed(serializeLayout(layout))).toEqual(layout);
  });
});
