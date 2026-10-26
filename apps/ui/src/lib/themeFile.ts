import { InvalidKdlError, getLocation, parse, type Node } from '@bgotink/kdl';
import type { LayoutError } from './layout/parse';
import { THEMES, type Theme } from './themes';

export const THEME_VERSION = 1;

const OPAQUE = ['bg', 'surface', 'surface-2', 'border', 'divider', 'text', 'text-muted'] as const;
const TOKENS = [...OPAQUE, 'hover', 'shadow'] as const;
const FONTS = ['font-sans', 'font-mono'] as const;

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;
const HEX_ALPHA = /^#(?:[0-9a-f]{4}|[0-9a-f]{8})$/i;
// Letters, digits, spaces, commas, quotes and hyphens only, so a font can't smuggle CSS in.
const FONT_LIST = /^[\w ,'"-]{1,200}$/;
// Offsets, blur and colours only; `/` and `.` are for alpha and fractions.
const SHADOW = /^[\w #(),./ -]{1,120}$/;

export type ThemeParse = { ok: true; theme: Theme; warnings: string[] } | { ok: false; errors: LayoutError[] };

const list = (items: readonly string[]) => items.map((i) => `\`${i}\``).join(', ');

/**
 * Parses and validates a theme file, collecting every error. Tokens left out
 * come from the built-in light or dark theme, so a file only needs to say
 * what it changes. Poor contrast doesn't fail the file; it comes back as
 * `warnings`.
 */
export function parseTheme(text: string, fallbackName = 'Untitled'): ThemeParse {
  let nodes: Node[];
  try {
    nodes = parse(text, { storeLocations: true }).nodes;
  } catch (e) {
    if (!(e instanceof InvalidKdlError)) throw e;
    const errors = e.errors?.length ? e.errors : [e];
    return {
      ok: false,
      errors: errors.map((err) => ({ message: err.message, line: err.start?.line ?? null, column: err.start?.column ?? null })),
    };
  }

  const errors: LayoutError[] = [];
  const error = (node: Node | undefined, message: string) => {
    const start = node && getLocation(node)?.start;
    errors.push({ message, line: start?.line ?? null, column: start?.column ?? null });
  };

  const [header, ...rest] = nodes;
  if (header?.getName() !== 'theme') {
    error(header, `the file must start with \`theme version=${THEME_VERSION} scheme="dark" name="…"\``);
    return { ok: false, errors };
  }
  for (const prop of header.getProperties().keys()) {
    if (!['version', 'name', 'scheme', ...FONTS].includes(prop)) {
      error(header, `\`theme\` has no property \`${prop}\`; expected ${list(['version', 'name', 'scheme', ...FONTS])}`);
    }
  }
  if (header.getArguments().length) error(header, '`theme` takes no arguments');
  const version = header.getProperty('version');
  if (version !== THEME_VERSION) {
    error(header, `unsupported theme version ${String(version)}; this app reads version ${THEME_VERSION}`);
  }
  const scheme = header.getProperty('scheme');
  if (scheme !== 'light' && scheme !== 'dark') {
    error(header, '`scheme` must be "light" or "dark" (it sets form controls and scrollbars)');
  }
  const prop = (key: string) => {
    const value = header.getProperty(key);
    if (value !== undefined && typeof value !== 'string') error(header, `\`${key}\` must be text`);
    return typeof value === 'string' ? value : undefined;
  };
  const name = prop('name') || fallbackName;

  const base = THEMES.find((t) => t.scheme === scheme) ?? THEMES[0];
  const tokens: Record<string, string> = { ...base.tokens };
  const seen = new Set<string>();
  for (const node of rest) {
    const key = node.getName();
    if (!(TOKENS as readonly string[]).includes(key)) {
      error(node, `unknown token \`${key}\`; expected one of ${list(TOKENS)}`);
      continue;
    }
    if (seen.has(key)) error(node, `\`${key}\` is set twice`);
    seen.add(key);
    const args = node.getArguments();
    const value = args[0];
    if (args.length !== 1 || typeof value !== 'string' || node.getProperties().size || node.children) {
      error(node, `\`${key}\` takes one text value, e.g. \`${key} "#1a1a1a"\``);
    } else if (key === 'shadow' ? !SHADOW.test(value) : key === 'hover' ? !HEX.test(value) && !HEX_ALPHA.test(value) : !HEX.test(value)) {
      const want = key === 'shadow' ? 'a CSS box-shadow such as "0 8px 32px rgb(0 0 0 / 0.5)"' : key === 'hover' ? 'a hex colour such as "#ffffff0d"' : 'a hex colour such as "#1a1a1a"';
      error(node, `\`${key}\` must be ${want}, got "${value}"`);
    } else {
      tokens[key] = value;
    }
  }

  const sans = prop('font-sans');
  const mono = prop('font-mono');
  for (const [key, value] of [['font-sans', sans], ['font-mono', mono]] as const) {
    if (value !== undefined && !FONT_LIST.test(value)) {
      error(header, `\`${key}\` must be a comma-separated list of font names, got "${value}"`);
    }
  }

  if (errors.length) return { ok: false, errors };
  const theme: Theme = {
    id: '',
    name,
    scheme: scheme as Theme['scheme'],
    tokens: tokens as Theme['tokens'],
    fonts: { sans, mono },
  };
  return { ok: true, theme, warnings: contrastWarnings(theme) };
}

function luminance(hex: string): number {
  let h = hex.slice(1);
  if (h.length === 3) h = [...h].map((c) => c + c).join('');
  const [r, g, b] = [0, 2, 4].map((i) => {
    const c = parseInt(h.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio of two opaque hex colours, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

/** Text that is hard to read on the surfaces it sits on. */
export function contrastWarnings({ tokens: t }: Theme): string[] {
  const out: string[] = [];
  for (const [fg, min] of [['text', 4.5], ['text-muted', 3]] as const) {
    for (const bg of ['bg', 'surface', 'surface-2'] as const) {
      const ratio = contrast(t[fg], t[bg]);
      if (ratio < min) out.push(`\`${fg}\` on \`${bg}\` has contrast ${ratio.toFixed(1)}:1; aim for at least ${min}:1`);
    }
  }
  return out;
}

/** Whether a file's first node is `theme`, so dropped files can be routed. */
export const isThemeFile = (text: string) => /^(?:\s|\/\/[^\n]*)*theme[\s{]/.test(text);
