import { getContext, setContext } from 'svelte';
import type { EngineClient, PlaybackPosition } from './engine.svelte';
import type { LibraryClient } from './library.svelte';
import type { BrowseState, LayoutStore, Source } from './layout/store.svelte';
import type { SettingsStore } from './settings.svelte';
import type { SettingsView } from './settingsView.svelte';
import type { PlayerSheet } from './sheet.svelte';
import type { UserThemes } from './userThemes.svelte';
import type { Views } from './views.svelte';

/** App-wide objects shared through Svelte context instead of prop drilling. */
export interface AppContext {
  engine: EngineClient;
  library: LibraryClient;
  position: PlaybackPosition;
  layout: LayoutStore;
  settings: SettingsStore;
  themes: UserThemes;
  sheet: PlayerSheet;
  settingsView: SettingsView;
  views: Views;
}

/** State shared by the widgets of one music area. */
export interface ViewState {
  source: Source;
  /** For per-layout preferences such as column widths. */
  layoutId: string;
  /** Selection and album focus, shared by every layout of this source. */
  browse: BrowseState;
}

const APP = Symbol('app');
const VIEW = Symbol('view');

export const setApp = (ctx: AppContext) => setContext(APP, ctx);
export const getApp = () => getContext<AppContext>(APP);
export const setViewState = (state: ViewState) => setContext(VIEW, state);
export const getViewState = () => getContext<ViewState>(VIEW);
