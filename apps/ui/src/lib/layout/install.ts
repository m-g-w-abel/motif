import { toasts } from '../toast.svelte';
import type { LayoutStore } from './store.svelte';

/** Installs dropped or picked `.kdl` files, reporting the outcome as toasts. */
export async function installLayoutFiles(store: LayoutStore, files: Iterable<File>) {
  for (const file of files) {
    if (!/\.kdl$/i.test(file.name)) {
      toasts.show(`${file.name} isn't a layout file (expected .kdl)`, 'error');
      continue;
    }
    const result = store.install(await file.text(), file.name);
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
