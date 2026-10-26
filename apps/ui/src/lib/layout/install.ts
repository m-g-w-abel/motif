import { toasts } from '../toast.svelte';
import { isThemeFile } from '../themeFile';
import type { UserThemes } from '../userThemes.svelte';
import type { LayoutStore } from './store.svelte';

/** Installs dropped or picked `.kdl` files, reporting the outcome as toasts. Files starting with a `theme` node go to `themes`. */
export async function installLayoutFiles(store: LayoutStore, files: Iterable<File>, themes?: UserThemes) {
  for (const file of files) {
    if (!/\.kdl$/i.test(file.name)) {
      toasts.show(`${file.name} isn't a layout file (expected .kdl)`, 'error');
      continue;
    }
    const text = await file.text();
    if (themes && isThemeFile(text)) {
      installTheme(themes, text, file.name);
      continue;
    }
    const result = store.install(text, file.name);
    if (result.ok) {
      toasts.show(`Installed layout “${result.entry.layout.name}”`);
    } else {
      const [first, ...rest] = result.errors;
      const where = first.line !== null ? `line ${first.line}: ` : '';
      const more = rest.length ? ` (+${rest.length} more)` : '';
      toasts.show(`Couldn't install ${file.name}: ${where}${first.message}${more}`, 'error');
    }
  }
}

/** Installs a theme file's text, reporting the outcome (and any contrast warnings) as toasts. */
export function installTheme(themes: UserThemes, text: string, fileName: string): string | null {
  const result = themes.install(text, fileName);
  if (!result.ok) {
    const [first, ...rest] = result.errors;
    const where = first.line !== null ? `line ${first.line}: ` : '';
    const more = rest.length ? ` (+${rest.length} more)` : '';
    toasts.show(`Couldn't install ${fileName}: ${where}${first.message}${more}`, 'error');
    return null;
  }
  toasts.show(`Installed theme “${result.theme.name}”`);
  if (result.warnings.length) {
    const more = result.warnings.length > 1 ? ` (+${result.warnings.length - 1} more)` : '';
    toasts.show(`Hard to read: ${result.warnings[0]}${more}`, 'error');
  }
  return result.id ?? null;
}
