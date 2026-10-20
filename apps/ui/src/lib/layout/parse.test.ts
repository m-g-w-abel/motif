import { describe, expect, it } from 'vitest';
import { builtinLayouts } from './presets';
import { parseLayout, type LayoutError } from './parse';

function errorsOf(text: string): LayoutError[] {
  const result = parseLayout(text);
  if (result.ok) throw new Error('expected the layout to be rejected');
  return result.errors;
}

describe('parseLayout', () => {
  it.each(builtinLayouts.map((l) => [l.fileName, l.text]))('accepts the built-in layout %s', (_, text) => {
    const result = parseLayout(text);
    expect(result.ok ? [] : result.errors).toEqual([]);
  });

  it('reads the header and builds the widget tree', () => {
    const result = parseLayout(`
      layout version=1 name="Mine" description="Just mine" icon=info
      split row {
        tracklist style=grouped max-width=900 size="60%"
        split column size=300 {
          artwork
          track-info style=table
        }
      }
    `);
    expect(result).toEqual({
      ok: true,
      layout: {
        version: 1,
        name: 'Mine',
        description: 'Just mine',
        icon: 'info',
        root: {
          kind: 'split',
          direction: 'row',
          size: undefined,
          children: [
            {
              kind: 'panel',
              panel: 'tracklist',
              size: { percent: 60 },
              style: 'grouped',
              columns: ['index', 'artist', 'title', 'duration'],
              maxWidth: 900,
            },
            {
              kind: 'split',
              direction: 'column',
              size: { px: 300 },
              children: [
                { kind: 'panel', panel: 'artwork', size: undefined },
                { kind: 'panel', panel: 'track-info', size: undefined, style: 'table' },
              ],
            },
          ],
        },
      },
    });
  });

  it('names unnamed layouts after the file and stacks several widgets', () => {
    const result = parseLayout('layout version=1\ntracklist\nalbum-grid card-size=150', 'from-file');
    expect(result.ok && result.layout.name).toBe('from-file');
    expect(result.ok && result.layout.root).toMatchObject({
      kind: 'split',
      direction: 'column',
      children: [{ panel: 'tracklist', style: 'table' }, { panel: 'album-grid', cardSize: 150 }],
    });
  });

  it('configures the controls and queue widgets', () => {
    const result = parseLayout('layout version=1\ncontrols\nqueue collapsible=#true');
    expect(result.ok && result.layout.root).toMatchObject({
      children: [{ panel: 'controls' }, { panel: 'queue', collapsible: true }],
    });
    expect(errorsOf('layout version=1\nqueue collapsible=yes')[0].message).toBe(
      '`collapsible` must be #true or #false, got "yes"',
    );
  });

  it('reports syntax errors with their line', () => {
    const [error] = errorsOf('layout version=1\nsplit row {\n  queue size=\n}');
    expect(error.line).toBe(3);
  });

  it('collects every validation error with locations', () => {
    const errors = errorsOf(
      [
        'layout version=1 icon=star',
        'split row {',
        '  tracklist colour=red {',
        '    columns title rating',
        '  }',
        '  sidebar',
        '}',
      ].join('\n'),
    );
    expect(errors.map((e) => [e.line, e.message])).toEqual([
      [1, '`icon` must be one of `list`, `grid`, `split`, `info`, `columns`, got "star"'],
      [3, '`tracklist` has no property `colour`; expected `style`, `max-width`, `size`'],
      [4, 'unknown column `rating`; expected one of `index`, `title`, `artist`, `album`, `format`, `duration`'],
      [6, 'unknown widget `sidebar`; expected one of `split`, `tracklist`, `album-grid`, `artwork`, `track-info`, `controls`, `queue`'],
    ]);
  });

  it('requires the header and some content', () => {
    expect(errorsOf('tracklist').map((e) => e.message)).toEqual([
      'the file must start with `layout version=1 name="…"`',
    ]);
    expect(errorsOf('layout version=1').map((e) => e.message)).toEqual([
      'the layout is empty; add a widget such as `tracklist`',
    ]);
  });

  it('rejects bad sizes and pixel values', () => {
    const messages = errorsOf('layout version=1\nqueue size="150%"\nalbum-grid card-size=20').map((e) => e.message);
    expect(messages).toEqual([
      'invalid size "150%"; use pixels (`size=240`) or a percentage (`size="30%"`)',
      '`card-size` must be a number of pixels between 80 and 600, got 20',
    ]);
  });
});
