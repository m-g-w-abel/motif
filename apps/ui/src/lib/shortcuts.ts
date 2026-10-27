export type ShortcutGroup = 'playback' | 'navigation' | 'lists' | 'search';

export const SHORTCUT_GROUPS: { id: ShortcutGroup; label: string }[] = [
  { id: 'playback', label: 'Playback' },
  { id: 'navigation', label: 'Navigation' },
  { id: 'lists', label: 'Lists and grids' },
  { id: 'search', label: 'Search' },
];

/** A key press. `shift` left undefined means "either" (the `?` key needs Shift on some layouts and not others). */
export interface Binding {
  key: string;
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
}

export interface Shortcut {
  id: string;
  group: ShortcutGroup;
  label: string;
  /** Shown in the help, one entry per alternative. */
  keys: string[];
  /** Keys the app-wide handler runs. Absent when a component handles its own keys; the entry then only documents them. */
  bindings?: Binding[];
}

/** The one table behind the `?` cheat sheet, Settings → Keyboard and the app-wide handler. */
export const SHORTCUTS: Shortcut[] = [
  { id: 'play_pause', group: 'playback', label: 'Play / pause', keys: ['Space'], bindings: [{ key: ' ', shift: false }] },
  { id: 'seek_back', group: 'playback', label: 'Seek back 5 seconds', keys: ['←'], bindings: [{ key: 'ArrowLeft', shift: false }] },
  { id: 'seek_forward', group: 'playback', label: 'Seek forward 5 seconds', keys: ['→'], bindings: [{ key: 'ArrowRight', shift: false }] },
  { id: 'previous', group: 'playback', label: 'Previous track', keys: ['Shift+←'], bindings: [{ key: 'ArrowLeft', shift: true }] },
  { id: 'next', group: 'playback', label: 'Next track', keys: ['Shift+→'], bindings: [{ key: 'ArrowRight', shift: true }] },

  { id: 'help', group: 'navigation', label: 'Show this help', keys: ['?'], bindings: [{ key: '?' }] },
  { id: 'settings', group: 'navigation', label: 'Open or close Settings', keys: ['Ctrl+,'], bindings: [{ key: ',', ctrl: true }] },
  { id: 'toggle_nav', group: 'navigation', label: 'Fold the left panel', keys: ['Ctrl+B'] },
  { id: 'escape', group: 'navigation', label: 'Close the menu, sheet, help or Settings', keys: ['Esc'] },

  { id: 'list_move', group: 'lists', label: 'Move between rows and cards', keys: ['↑', '↓', '←', '→'] },
  { id: 'list_page', group: 'lists', label: 'Page up / down', keys: ['PageUp', 'PageDown'] },
  { id: 'list_ends', group: 'lists', label: 'Start / end of the row (Ctrl: of the list)', keys: ['Home', 'End'] },
  { id: 'list_play', group: 'lists', label: 'Play the row or album', keys: ['Enter'] },
  { id: 'list_remove', group: 'lists', label: 'Remove from the queue', keys: ['Delete', 'Backspace'] },
  { id: 'list_reorder', group: 'lists', label: 'Move an item up / down in a reorderable list', keys: ['Alt+↑', 'Alt+↓'] },
  { id: 'context_menu', group: 'lists', label: 'Open the context menu', keys: ['Menu', 'Shift+F10'] },

  { id: 'search', group: 'search', label: 'Search the library', keys: ['/', 'Ctrl+F'], bindings: [{ key: '/' }, { key: 'f', ctrl: true }] },
];

/** For later modes (Vim) to add their keys to the same help. */
export function registerShortcuts(...shortcuts: Shortcut[]) {
  SHORTCUTS.push(...shortcuts);
}

type Press = Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>;

/** The id of the shortcut with a binding for this key press, if any. */
export function shortcutFor(e: Press): string | null {
  const hit = SHORTCUTS.find((s) =>
    s.bindings?.some(
      (b) =>
        b.key === e.key &&
        !!b.ctrl === (e.ctrlKey || e.metaKey) &&
        !!b.alt === e.altKey &&
        (b.shift === undefined || b.shift === e.shiftKey),
    ),
  );
  return hit?.id ?? null;
}
