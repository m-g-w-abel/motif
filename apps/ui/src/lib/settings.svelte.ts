/**
 * Appearance and player preferences. Client-side and per device; layouts
 * don't touch these.
 */
export interface Settings {
  /** A theme id from themes.ts, or `system` to follow the OS light/dark mode. */
  theme: string;
  accent: string;
  /** Take the accent from the playing album's art instead. */
  artAccent: boolean;
  density: 'compact' | 'comfortable';
  /** Interface scale in percent, 90 to 150. */
  scale: number;
  /** A font family list that overrides the theme's; empty keeps the theme's (or the app's). */
  font: string;
  /** The always-visible player: a bar along the bottom, or a floating widget. */
  player: 'bar' | 'floating';
  /** Desktop: the left panel folded away. */
  navCollapsed: boolean;
  /** Vim-style list navigation (j/k, gg/G, Ctrl-d/u, counts). */
  vimKeys: boolean;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: 'graphite',
  accent: '#e8a33d',
  artAccent: false,
  density: 'compact',
  scale: 100,
  font: '',
  player: 'bar',
  navCollapsed: false,
  vimKeys: false,
};

const KEY = 'settings';

export class SettingsStore {
  current = $state<Settings>({ ...DEFAULT_SETTINGS });

  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
      // Earlier versions stored a plain light/dark mode.
      if (saved.theme === 'dark') saved.theme = 'graphite';
      this.current = { ...DEFAULT_SETTINGS, ...saved };
    } catch {
      // Defaults.
    }
  }

  set<K extends keyof Settings>(key: K, value: Settings[K]) {
    this.current[key] = value;
    try {
      localStorage.setItem(KEY, JSON.stringify(this.current));
    } catch {
      // Applies until reload.
    }
  }
}
