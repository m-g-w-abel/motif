import { parseTheme, type ThemeParse } from './themeFile';
import type { Theme } from './themes';

// Like layouts, user themes live in this device's storage as their source text.
const KEY = 'themes.user';

interface Stored {
  id: string;
  text: string;
}

const slug = (name: string) =>
  name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'theme';

export class UserThemes {
  #stored = $state.raw<Stored[]>([]);

  list: Theme[] = $derived(
    this.#stored.flatMap(({ id, text }) => {
      const result = parseTheme(text);
      return result.ok ? [{ ...result.theme, id }] : [];
    }),
  );

  constructor() {
    try {
      this.#stored = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    } catch {
      // None.
    }
  }

  byId(id: string): Theme | undefined {
    return this.list.find((t) => t.id === id);
  }

  /** Adds a theme, replacing an installed one of the same name. */
  install(text: string, fileName: string): ThemeParse & { id?: string } {
    const result = parseTheme(text, fileName.replace(/\.kdl$/i, ''));
    if (!result.ok) return result;
    const id = `user:${slug(result.theme.name)}`;
    this.#set([...this.#stored.filter((s) => s.id !== id), { id, text }]);
    return { ...result, theme: { ...result.theme, id }, id };
  }

  remove(id: string) {
    this.#set(this.#stored.filter((s) => s.id !== id));
  }

  #set(stored: Stored[]) {
    this.#stored = stored;
    try {
      localStorage.setItem(KEY, JSON.stringify(stored));
    } catch {
      // Applies until reload.
    }
  }
}
