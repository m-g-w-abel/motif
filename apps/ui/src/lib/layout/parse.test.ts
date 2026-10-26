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
              albumBreaks: true,
              headers: false,
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

  it('takes tracklist options independently of the style shorthand', () => {
    const panel = (props: string) => {
      const r = parseLayout(`layout version=1\ntracklist ${props}`, 'x');
      if (!r.ok) throw new Error(JSON.stringify(r.errors));
      return r.layout.root;
    };
    expect(panel('')).toMatchObject({ albumBreaks: false, headers: true });
    expect(panel('style=grouped')).toMatchObject({ albumBreaks: true, headers: false, columns: ['index', 'artist', 'title', 'duration'] });
    expect(panel('style=grouped headers=#true')).toMatchObject({ albumBreaks: true, headers: true });
    expect(panel('album-breaks=#true')).toMatchObject({ albumBreaks: true, headers: true });
    expect(panel('style=table headers=#false')).toMatchObject({ albumBreaks: false, headers: false });
    expect(errorsOf('layout version=1\ntracklist headers=yes').map((e) => e.message)).toEqual(['`headers` must be #true or #false, got "yes"']);
  });

  it('names unnamed layouts after the file and stacks several widgets', () => {
    const result = parseLayout('layout version=1\ntracklist\nalbum-grid card-size=150', 'from-file');
    expect(result.ok && result.layout.name).toBe('from-file');
    expect(result.ok && result.layout.root).toMatchObject({
      kind: 'split',
      direction: 'column',
      children: [{ panel: 'tracklist', albumBreaks: false, headers: true }, { panel: 'album-grid', cardSize: 150 }],
    });
  });

  it('reads and validates album-grid columns', () => {
    const result = parseLayout('layout version=1\nalbum-grid columns=4');
    expect(result.ok && result.layout.root).toMatchObject({ panel: 'album-grid', columns: 4 });
    const none = parseLayout('layout version=1\nalbum-grid');
    expect(none.ok && (none.layout.root as { columns?: number }).columns).toBeUndefined();
    expect(errorsOf('layout version=1\nalbum-grid columns=0')[0].message).toBe(
      '`columns` must be a whole number between 1 and 24, got 0',
    );
    expect(errorsOf('layout version=1\nalbum-grid columns=2.5')).toHaveLength(1);
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
      [3, '`tracklist` has no property `colour`; expected `style`, `album-breaks`, `headers`, `max-width`, `size`'],
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

  it('reads a phone variant', () => {
    const result = parseLayout(`
      layout version=1
      split row {
        tracklist
        artwork size=300
      }
      when phone {
        artwork size="40%"
        tracklist
      }
    `);
    if (!result.ok) throw new Error(JSON.stringify(result.errors));
    expect(result.layout.root).toMatchObject({ kind: 'split', direction: 'row' });
    expect(result.layout.phone).toEqual({
      kind: 'split',
      direction: 'column',
      children: [
        { kind: 'panel', panel: 'artwork', size: { percent: 40 } },
        { kind: 'panel', panel: 'tracklist', albumBreaks: false, headers: true, columns: ['index', 'title', 'artist', 'album', 'duration'] },
      ],
    });
  });

  it('leaves out the phone variant when there is none', () => {
    const result = parseLayout('layout version=1\ntracklist');
    expect(result.ok && result.layout.phone).toBeUndefined();
  });

  it('rejects bad variants', () => {
    const messages = (text: string) => errorsOf(`layout version=1\n${text}`).map((e) => e.message);
    expect(messages('tracklist\nwhen tablet { tracklist }')).toEqual(['`when` takes a device: `when phone { ... }`']);
    expect(messages('tracklist\nwhen phone { queue }\nwhen phone { queue }')).toEqual([
      '`when phone` can only appear once',
    ]);
    expect(messages('tracklist\nwhen phone')).toEqual(['`when phone` is empty']);
    expect(messages('split row { when phone { queue } }')).toEqual([
      '`when` can only appear at the top of the file, not inside a widget',
    ]);
    expect(messages('when phone { queue }')).toEqual([
      'add widgets outside `when phone` too; they are used on wider screens',
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
