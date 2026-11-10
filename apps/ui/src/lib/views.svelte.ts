import { MediaQuery } from 'svelte/reactivity';
import type { IconName } from './components/Icon.svelte';

/**
 * What the music area shows. All Songs (`songs`) is the library view that
 * desktop layouts arrange; the others show one widget each.
 */
export type View =
  | { kind: 'home' }
  | { kind: 'songs' }
  | { kind: 'albums' }
  | { kind: 'playlists' }
  | { kind: 'played' }
  | { kind: 'added' }
  | { kind: 'album'; key: string; title: string; art?: string | null }
  | { kind: 'playlist'; id: string; name: string }
  | { kind: 'search' };

export type Category = Extract<View['kind'], 'songs' | 'albums' | 'playlists' | 'played' | 'added'>;

/** The Library home's entries, in order, each with its round icon's colour. */
export const CATEGORIES: { kind: Category; label: string; icon: IconName; colour: string }[] = [
  { kind: 'songs', label: 'All Songs', icon: 'music', colour: '#6f8fd8' },
  { kind: 'albums', label: 'Albums', icon: 'grid', colour: '#5a55b8' },
  { kind: 'playlists', label: 'Playlists', icon: 'list', colour: '#7a4638' },
  { kind: 'played', label: 'Recently Played', icon: 'play', colour: '#3f3d8a' },
  { kind: 'added', label: 'Recently Added', icon: 'plus', colour: '#4e6043' },
];

const HOME: View = { kind: 'home' };

export function viewTitle(view: View): string {
  if (view.kind === 'home') return 'Library';
  if (view.kind === 'search') return 'Search';
  if (view.kind === 'album') return view.title || 'Unknown album';
  if (view.kind === 'playlist') return view.name;
  return CATEGORIES.find((c) => c.kind === view.kind)!.label;
}

function same(a: View, b: View): boolean {
  if (a.kind === 'album') return b.kind === 'album' && a.key === b.key;
  if (a.kind === 'playlist') return b.kind === 'playlist' && a.id === b.id;
  return a.kind === b.kind;
}

/** The stack of views, the top one showing; back pops it. */
export class Views {
  stack = $state.raw<View[]>([HOME]);
  /** The top view's ⋮ sheet is open; any move to another view closes it. */
  options = $state(false);
  /** The top view's sort menu is open (as `options`); kept here so system back closes it first. */
  sorting = $state(false);
  // No window under unit tests.
  #narrow = typeof window === 'undefined' ? { current: false } : new MediaQuery('(max-width: 700px)');
  /** Phone width: views fill the music area and the Library home is one of them, not the left panel. */
  get phone() {
    return this.#narrow.current;
  }
  top: View = $derived(this.stack.at(-1)!);
  /** The phone bar's button the views are under (highlighted there). */
  root: 'library' | 'search' = $derived(this.stack[1]?.kind === 'search' ? 'search' : 'library');
  /** The phone bar's menu sheet: its main list, or the engine switcher in its place. */
  menu = $state<'main' | 'engines' | null>(null);
  /** The changelog dialog is open (from the phone menu or the left panel). */
  changelog = $state(false);
  /** Where back goes; the Library home is under every view, even when it isn't on the stack. */
  parent: View | null = $derived(this.stack.at(-2) ?? (this.top.kind === 'home' ? null : HOME));

  /** Opens `view` above the current one; safe to repeat. */
  open(view: View) {
    if (!same(view, this.top)) this.stack = [...this.stack, view];
    this.options = this.sorting = false;
  }

  /** Starts again from the Library home with `view` above it (and an album's or playlist's category between). */
  show(view: View) {
    this.menu = null;
    this.stack = stackOf(view);
    this.options = this.sorting = false;
  }

  /** Goes back a view. False when already at the Library home. */
  back(): boolean {
    if (!this.parent) return false;
    this.stack = this.stack.length > 1 ? this.stack.slice(0, -1) : [HOME];
    this.options = this.sorting = false;
    return true;
  }
}

/** The stack a view opens afresh with: the Library home, then the view's category. */
function stackOf(view: View): View[] {
  if (view.kind === 'home') return [HOME];
  if (view.kind === 'album') return [HOME, { kind: 'albums' }, view];
  if (view.kind === 'playlist') return [HOME, { kind: 'playlists' }, view];
  return [HOME, view];
}

/** The stack `?dev-view=` opens: a category (`albums`), `search`, `album:<key>` or `playlist:<id>`. */
export function devViews(param: string): View[] | null {
  if (param.startsWith('album:')) {
    const key = param.slice('album:'.length);
    return key ? stackOf({ kind: 'album', key, title: key }) : null;
  }
  if (param.startsWith('playlist:')) {
    const id = param.slice('playlist:'.length);
    return id ? stackOf({ kind: 'playlist', id, name: id }) : null;
  }
  const kind = param === 'search' ? 'search' : CATEGORIES.find((c) => c.kind === param)?.kind;
  return kind ? stackOf({ kind }) : null;
}

/** The link a launcher shortcut opens `view` with (`MainActivity.kt`), e.g. `motif://view/album?key=…&title=…`. */
export function viewLink(view: View): string {
  const params: Record<string, string> =
    view.kind === 'album' ? { key: view.key, title: view.title } : view.kind === 'playlist' ? { id: view.id, name: view.name } : {};
  const query = new URLSearchParams(params).toString();
  return `motif://view/${view.kind}${query ? `?${query}` : ''}`;
}

/** The view a `viewLink` opens, or null if `text` is another link. */
export function parseViewLink(text: string): View | null {
  if (!text.startsWith('motif://view/')) return null;
  const url = new URL(text);
  const kind = url.pathname.slice(1);
  const get = (name: string) => url.searchParams.get(name);
  const key = get('key');
  const id = get('id');
  if (kind === 'album') return key ? { kind, key, title: get('title') ?? '' } : null;
  if (kind === 'playlist') return id ? { kind, id, name: get('name') ?? '' } : null;
  if (kind === 'home') return HOME;
  return devViews(kind)?.at(-1) ?? null;
}
