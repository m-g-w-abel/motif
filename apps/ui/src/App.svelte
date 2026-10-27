<script lang="ts">
  import { onDestroy, tick, type Component } from 'svelte';
  import { setApp } from './lib/context';
  import { demoLibrary } from './lib/demo';
  import { Drawer } from './lib/drawer.svelte';
  import { SETTINGS_GROUPS, SettingsView, type SettingsGroup } from './lib/settingsView.svelte';
  import SettingsScreen from './lib/components/shell/SettingsScreen.svelte';
  import { EngineClient, PlaybackPosition } from './lib/engine.svelte';
  import { accentFromPixels } from './lib/artAccent';
  import { LibraryClient } from './lib/library.svelte';
  import { installLayoutFiles } from './lib/layout/install';
  import { LayoutStore } from './lib/layout/store.svelte';
  import { SettingsStore } from './lib/settings.svelte';
  import { SYSTEM_THEMES, themeById } from './lib/themes';
  import { FONT_LIST } from './lib/themeFile';
  import { UserThemes } from './lib/userThemes.svelte';
  import { MediaQuery } from 'svelte/reactivity';
  import { PlayerSheet } from './lib/sheet.svelte';
  import CopyButton from './lib/components/CopyButton.svelte';
  import { toasts } from './lib/toast.svelte';
  import { shortcutFor } from './lib/shortcuts';
  import ContextMenu from './lib/components/ContextMenu.svelte';
  import ShortcutHelp from './lib/components/shell/ShortcutHelp.svelte';
  import LayoutEditor from './lib/components/shell/LayoutEditor.svelte';
  import ContentArea from './lib/components/shell/ContentArea.svelte';
  import Nav from './lib/components/shell/Nav.svelte';
  import ExpandedPlayer from './lib/components/player/ExpandedPlayer.svelte';
  import FloatingPlayer from './lib/components/player/FloatingPlayer.svelte';
  import NowPlayingBar from './lib/components/player/NowPlayingBar.svelte';

  const engine = new EngineClient();
  const position = new PlaybackPosition(engine);
  const library = new LibraryClient(engine);
  const layout = new LayoutStore();
  const themes = new UserThemes();
  const settings = new SettingsStore();
  const sheet = new PlayerSheet();
  const drawer = new Drawer();
  const settingsView = new SettingsView();
  setApp({ engine, library, position, layout, themes, settings, sheet, drawer, settingsView });

  // On phones the left panel is a drawer: swipe in from the left edge to open
  // it, swipe left to close it.
  function onPointerDown(e: PointerEvent) {
    if (window.innerWidth <= 700 && !sheet.open) drawer.swipe(e);
  }
  onDestroy(() => engine.close());

  $effect(() => position.track());

  // Apply appearance settings to the document: the theme's colour tokens,
  // the accent and the density.
  const prefersDark = new MediaQuery('(prefers-color-scheme: dark)');
  const themeId = $derived(
    settings.current.theme === 'system'
      ? SYSTEM_THEMES[prefersDark.current ? 'dark' : 'light']
      : settings.current.theme,
  );
  // A removed or broken user theme falls back to the first built-in.
  const theme = $derived(themes.byId(themeId) ?? themeById(themeId));
  $effect(() => {
    const { density, scale, font } = settings.current;
    const accent = artColor ?? settings.current.accent;
    const root = document.documentElement;
    for (const [token, value] of Object.entries(theme.tokens)) root.style.setProperty(`--${token}`, value);
    root.style.colorScheme = theme.scheme;
    // The user's own font beats the theme's, which beats the app's.
    for (const [prop, themed] of [['--font-sans', theme.fonts?.sans], ['--font-mono', theme.fonts?.mono]] as const) {
      const value = (prop === '--font-sans' && FONT_LIST.test(font) && font) || themed;
      if (value) root.style.setProperty(prop, value);
      else root.style.removeProperty(prop);
    }
    // Zoom on the root scales everything, but media queries (the 700px phone
    // breakpoint) still see real pixels.
    // The shell is sized in viewport units, so it divides the zoom back out to fit the window.
    root.style.zoom = scale === 100 ? '' : `${scale}%`;
    root.style.setProperty('--scale', String(scale / 100));
    root.dataset.density = density;
    root.style.setProperty('--accent', accent);
    root.style.setProperty('--accent-contrast', contrastFor(accent));
  });

  // The accent from the playing track's art: read from a 64px thumbnail once
  // per track (and cached per art id), never while idle.
  let artColor = $state<string | null>(null);
  const artColors = new Map<string, string | null>();
  const artId = $derived(settings.current.artAccent ? library.details(engine.current?.track)?.art : null);
  $effect(() => {
    const id = artId;
    const bg = theme.tokens.bg;
    if (!id) {
      artColor = null;
      return;
    }
    const key = `${id} ${bg}`;
    if (artColors.has(key)) {
      artColor = artColors.get(key)!;
      return;
    }
    let stale = false;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 16;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      let color: string | null = null;
      try {
        ctx?.drawImage(img, 0, 0, 16, 16);
        color = accentFromPixels(ctx!.getImageData(0, 0, 16, 16).data, bg);
      } catch {
        // Tainted canvas (art served from another origin): keep the theme accent.
      }
      artColors.set(key, color);
      if (!stale) artColor = color;
    };
    img.onerror = () => !stale && (artColor = null);
    img.src = library.artUrl(id, 64);
    return () => (stale = true);
  });

  /** Black or white text, whichever reads better on the accent colour. */
  function contrastFor(color: string): string {
    const probe = document.createElement('span');
    probe.style.color = color;
    document.body.append(probe);
    const [r, g, b] = getComputedStyle(probe).color.match(/\d+(\.\d+)?/g)?.map(Number) ?? [0, 0, 0];
    probe.remove();
    return 0.299 * r + 0.587 * g + 0.114 * b > 150 ? '#111111' : '#ffffff';
  }

  // In-app shortcuts (the table is in lib/shortcuts.ts). Plain keys leave
  // buttons, sliders and list rows to their own handling.
  let helpOpen = $state(false);
  const typing = (t: EventTarget | null) => !!(t as Element | null)?.closest?.('input, textarea, select, [contenteditable="true"]');
  const focusable = (t: EventTarget | null) => !!(t as Element | null)?.closest?.('button, a, [tabindex]');

  async function onShortcut(e: KeyboardEvent) {
    if (e.defaultPrevented || helpOpen || typing(e.target)) return;
    const id = shortcutFor(e);
    if (!id || ((e.key === ' ' || e.key.startsWith('Arrow')) && focusable(e.target))) return;
    e.preventDefault();
    const seek = (by: number) => engine.run({ type: 'seek', position_ms: Math.max(0, position.ms + by) });
    switch (id) {
      case 'play_pause': engine.run({ type: 'toggle_playback' }); break;
      case 'seek_back': seek(-5000); break;
      case 'seek_forward': seek(5000); break;
      case 'previous': engine.run({ type: 'previous' }); break;
      case 'next': engine.run({ type: 'next' }); break;
      case 'help': helpOpen = true; break;
      case 'settings': if (settingsView.group) settingsView.close(); else settingsView.open(); break;
      case 'search':
        settingsView.close();
        await tick();
        document.querySelector<HTMLInputElement>('input[aria-label="Search the library"]')?.focus();
        break;
    }
  }

  // The app has its own interactions; the browser's context menu ("Inspect",
  // "Reload"…) only makes sense in text fields.
  function onContextMenu(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest('input, textarea, [contenteditable="true"]')) e.preventDefault();
  }

  // Dropping .kdl files anywhere installs them as layouts or themes.
  let dragDepth = $state(0);
  const hasFiles = (e: DragEvent) => e.dataTransfer?.types.includes('Files') ?? false;

  function onDragEnter(e: DragEvent) {
    if (hasFiles(e)) dragDepth++;
  }
  function onDragLeave(e: DragEvent) {
    if (hasFiles(e)) dragDepth = Math.max(0, dragDepth - 1);
  }
  function onDragOver(e: DragEvent) {
    if (hasFiles(e)) e.preventDefault();
  }
  function onDrop(e: DragEvent) {
    if (!hasFiles(e)) return;
    e.preventDefault();
    dragDepth = 0;
    void installLayoutFiles(layout, e.dataTransfer?.files ?? [], themes);
  }

  // Development only (stripped from builds): URL parameters that set up a
  // state for visual checks in other webviews, e.g.
  // ?dev-layout=builtin:details.kdl&dev-theme=light&dev-play=5&dev-sheet=1
  let StressTest = $state<Component | null>(null);
  if (import.meta.env.DEV) {
    const params = new URLSearchParams(location.search);
    const devLayout = params.get('dev-layout');
    if (devLayout) layout.select(devLayout);
    const devTheme = params.get('dev-theme');
    if (devTheme) settings.current.theme = devTheme;
    if (params.get('dev-player') === 'floating') settings.current.player = 'floating';
    if (params.has('dev-sheet')) sheet.expand();
    if (params.has('dev-drawer')) drawer.open = true;
    const devSettings = params.get('dev-settings') as SettingsGroup;
    if (SETTINGS_GROUPS.includes(devSettings)) settingsView.open(devSettings);
    if (params.has('dev-stress')) import('./lib/components/StressTest.svelte').then((m) => (StressTest = m.default));
    const devPlay = params.get('dev-play');
    if (devPlay !== null) {
      $effect(() => {
        if (engine.status === 'open') {
          engine.run({ type: 'set_context', tracks: demoLibrary, start: Number(devPlay), kind: 'list', label: 'Library' });
        }
      });
    }
  }

  let editing = $state(false);
  const player = $derived(settings.current.player);
</script>

<svelte:document oncontextmenu={onContextMenu} />
<svelte:window onkeydown={onShortcut} onpointerdown={onPointerDown} ondragenter={onDragEnter} ondragleave={onDragLeave} ondragover={onDragOver} ondrop={onDrop} />

<div class="shell">
  <Nav />

  <div class="body">
    {#if engine.error}
      <div class="error" role="alert">
        <span>{engine.error}</span>
        <CopyButton text={engine.error} />
        <button class="icon-button" aria-label="Dismiss" onclick={() => (engine.error = null)}>×</button>
      </div>
    {/if}

    <main>
      {#if settingsView.group}
        <SettingsScreen />
      {:else}
        {#key `${layout.source}/${layout.activeId}`}
          <ContentArea onEditLayout={() => (editing = true)} />
        {/key}
      {/if}
    </main>

    {#if player === 'bar'}<NowPlayingBar />{/if}

    <ExpandedPlayer />
  </div>

  <!-- Hidden, not unmounted, while the sheet is open: it collapses back into it. -->
  {#if player === 'floating'}<FloatingPlayer />{/if}
</div>

{#if helpOpen}<ShortcutHelp onClose={() => (helpOpen = false)} />{/if}

<ContextMenu />

{#if StressTest}<StressTest />{/if}

{#if editing}<LayoutEditor onClose={() => (editing = false)} />{/if}

{#if dragDepth > 0}
  <div class="drop-overlay" aria-hidden="true"><span>Drop a .kdl file to install it as a layout or theme</span></div>
{/if}

<div class="toasts" role="status">
  {#each toasts.items as toast (toast.id)}
    <div class="toast {toast.kind}">
      <button class="message selectable" onclick={() => !getSelection()?.toString() && toasts.dismiss(toast.id)}>{toast.message}</button>
      {#if toast.kind === 'error'}<CopyButton text={toast.message} />{/if}
    </div>
  {/each}
</div>

<style>
  .shell {
    display: flex;
    height: calc(100vh / var(--scale, 1));
    height: calc(100dvh / var(--scale, 1));
    padding: env(safe-area-inset-top) env(safe-area-inset-right) 0 env(safe-area-inset-left);
  }

  .body {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  main {
    flex: 1;
    min-height: 0;
  }

  .error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-1) var(--space-3);
    background: color-mix(in srgb, var(--danger) 18%, var(--surface));
    border-bottom: 1px solid var(--danger);
    flex: none;
  }

  .error span {
    flex: 1;
  }

  .drop-overlay {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    background: color-mix(in srgb, var(--bg) 80%, transparent);
    font-weight: 600;
    pointer-events: none;
  }

  .toasts {
    position: fixed;
    left: 50%;
    bottom: calc(var(--bar-height) + 16px);
    z-index: 60;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    transform: translateX(-50%);
    pointer-events: none;
  }

  .toast {
    max-width: min(520px, calc((100vw - 32px) / var(--scale, 1)));
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface);
    box-shadow: var(--shadow);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    pointer-events: auto;
  }

  .toast .message {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .toast.error {
    border-color: var(--danger);
  }

  @media (max-width: 700px) {
    .shell {
      flex-direction: column;
    }

    /* On phones the expanded player covers everything, tab bar included. */
    .body {
      position: static;
    }

    .toasts {
      bottom: 76px;
    }
  }
</style>
