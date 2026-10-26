import { describe, expect, it } from 'vitest';
import { contrast, isThemeFile, parseTheme } from './themeFile';
import { UserThemes } from './userThemes.svelte';

const ok = (text: string) => {
  const r = parseTheme(text);
  if (!r.ok) throw new Error(JSON.stringify(r.errors));
  return r;
};
const errors = (text: string) => {
  const r = parseTheme(text);
  if (r.ok) throw new Error('expected errors');
  return r.errors;
};

describe('parseTheme', () => {
  it('reads tokens and fonts, filling the rest from the built-in scheme', () => {
    const { theme, warnings } = ok(
      'theme version=1 name="Mine" scheme="dark" font-sans="Georgia, serif"\nbg "#101020"\nhover "#ffffff10"\n',
    );
    expect(theme.name).toBe('Mine');
    expect(theme.tokens.bg).toBe('#101020');
    expect(theme.tokens.text).toBe('#e7e7ea');
    expect(theme.fonts).toEqual({ sans: 'Georgia, serif', mono: undefined });
    expect(warnings).toEqual([]);
  });

  it('names the theme after the file when it has no name', () => {
    expect(parseTheme('theme version=1 scheme="light"', 'mine').ok && 'ok').toBe('ok');
    const r = parseTheme('theme version=1 scheme="light"', 'mine');
    expect(r.ok && r.theme.name).toBe('mine');
  });

  it('reports every problem with its line', () => {
    const found = errors(
      'theme version=2 scheme="blue" bogus=1\nbg "red"\nnope "#fff"\nsurface "#fff" "#000"\n',
    );
    const text = found.map((e) => `${e.line}: ${e.message}`).join('\n');
    expect(text).toContain('unsupported theme version 2');
    expect(text).toContain('`scheme` must be');
    expect(text).toContain('no property `bogus`');
    expect(text).toContain('2: `bg` must be a hex colour');
    expect(text).toContain('3: unknown token `nope`');
    expect(text).toContain('4: `surface` takes one text value');
  });

  it('rejects CSS smuggled into fonts and shadows', () => {
    expect(errors('theme version=1 scheme="dark" font-sans="a; background: url(x)"')[0].message).toContain('font-sans');
    expect(errors('theme version=1 scheme="dark"\nshadow "0 0 1px red; x: y"')[0].message).toContain('shadow');
  });

  it('rejects files that are not themes or not KDL', () => {
    expect(errors('layout version=1')[0].message).toContain('must start with `theme');
    expect(errors('theme {{{')[0].line).not.toBeUndefined();
  });

  it('warns, but still installs, when text is unreadable', () => {
    const { warnings } = ok('theme version=1 scheme="dark"\ntext "#222222"\n');
    expect(warnings.some((w) => w.includes('`text` on `bg`'))).toBe(true);
  });
});

describe('helpers', () => {
  it('computes WCAG contrast', () => {
    expect(contrast('#000000', '#ffffff')).toBeCloseTo(21);
    expect(contrast('#777', '#777')).toBe(1);
  });

  it('routes by top node, ignoring comments', () => {
    expect(isThemeFile('// hi\ntheme version=1')).toBe(true);
    expect(isThemeFile('layout version=1\ntheme')).toBe(false);
  });
});

describe('UserThemes', () => {
  it('installs, replaces by name and removes', () => {
    const themes = new UserThemes();
    const text = 'theme version=1 name="Mine" scheme="dark"\n';
    const r = themes.install(text, 'a.kdl');
    expect(r.ok && r.id).toBe('user:mine');
    themes.install(text + 'bg "#000"\n', 'b.kdl');
    expect(themes.list.map((t) => t.id)).toEqual(['user:mine']);
    expect(themes.byId('user:mine')?.tokens.bg).toBe('#000');
    themes.remove('user:mine');
    expect(themes.list).toEqual([]);
  });
});
