import type { ArtColours } from './artAccent';
import type { Action, Gesture, Overrides, Surface } from './gestures';
import { withOptions, type OptionsKey, type SavedOptions, type ViewOptions } from './viewOptions';

/**
 * Appearance and player preferences. Client-side and per device; layouts
 * don't touch these.
 */
export interface Settings {
  /** A theme id from themes.ts, or `system` to follow the OS light/dark mode. */
  theme: string;
  accent: string;
  /** How much of the playing album's art colours the interface. */
  artColours: ArtColours;
  density: 'compact' | 'comfortable';
  /** Interface scale in percent, 90 to 150. */
  scale: number;
  /** A font family list that overrides the theme's; empty keeps the theme's (or the app's). */
  font: string;
  /** The always-visible player: a bar along the bottom, or a floating widget. */
  player: 'bar' | 'floating';
  /** The full seek bar draws the track's waveform, or a plain line. */
  seekBar: 'waveform' | 'plain';
  /** Desktop: the left panel folded away. */
  navCollapsed: boolean;
  /** Vim-style list navigation (j/k, gg/G, Ctrl-d/u, counts). */
  vimKeys: boolean;
  /** Desktop: the app draws its own window buttons instead of the system title bar. */
  ownDecorations: boolean;
  /** Touch gestures remapped from their defaults (gestures.ts); empty keeps them all. */
  gestures: Overrides;
  /** Android: a remote engine's playback shows in the notification and lock screen. Off: only this phone's does. */
  remoteControls: boolean;
  /** Each view's display options (`viewOptions.ts`), only those changed. */
  viewOptions: SavedOptions;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: 'graphite',
  accent: '#e8a33d',
  artColours: 'off',
  density: 'compact',
  scale: 100,
  font: '',
  player: 'bar',
  seekBar: 'waveform',
  navCollapsed: false,
  vimKeys: false,
  ownDecorations: false,
  gestures: {},
  remoteControls: false,
  viewOptions: {},
};

const KEY = 'settings';

export class SettingsStore {
  current = $state<Settings>({ ...DEFAULT_SETTINGS });

  constructor() {
    try {
      const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
      // Earlier versions stored a plain light/dark mode.
      if (saved.theme === 'dark') saved.theme = 'graphite';
      // The first version was an on/off switch for the accent alone.
      if (saved.artAccent !== undefined) saved.artColours ??= saved.artAccent ? 'accent' : 'off';
      delete saved.artAccent;
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

  /** Changes some of a view's options, keeping the rest. */
  setView(key: OptionsKey, patch: Partial<ViewOptions>) {
    this.set('viewOptions', withOptions(this.current.viewOptions, key, patch));
  }

  /** Maps one gesture to `action`, keeping the others. */
  remap(surface: Surface, gesture: Gesture, action: Action | 'none') {
    const g = this.current.gestures;
    this.set('gestures', { ...g, [surface]: { ...g[surface], [gesture]: action } });
  }
}
