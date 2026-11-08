<script lang="ts">
  import { onDestroy, tick, untrack, type Component } from 'svelte';
  import { setApp } from './lib/context';
  import { demoLibrary } from './lib/demo';
  import { SETTINGS_GROUPS, SettingsView, type SettingsGroup } from './lib/settingsView.svelte';
  import SettingsScreen from './lib/components/shell/SettingsScreen.svelte';
  import { EngineClient, engines, PlaybackPosition } from './lib/engine.svelte';
  import { addEngine, LOCAL_ID, switchTo } from './lib/engines';
  import { accentFromPixels, artPalette } from './lib/artAccent';
  import { setBarIconsDark } from './lib/folders';
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
  import { routeVolumeKeys } from './lib/volumeKeys';
  import { remoteMedia, routeRemoteButtons, showRemoteMedia } from './lib/remoteMedia';
  import { routeBack } from './lib/back';
  import { parsePairLink } from './lib/discover';
  import { contextMenu } from './lib/contextMenu.svelte';
  import { applyDecorations } from './lib/windowChrome';
  import { shortcutFor } from './lib/shortcuts';
  import ResizeEdges from './lib/components/shell/ResizeEdges.svelte';
  import ContextMenu from './lib/components/ContextMenu.svelte';
  import ShortcutHelp from './lib/components/shell/ShortcutHelp.svelte';
  import LayoutEditor from './lib/components/shell/LayoutEditor.svelte';
  import ContentArea from './lib/components/shell/ContentArea.svelte';
  import ViewScreen from './lib/components/shell/ViewScreen.svelte';
  import { devViews, Views } from './lib/views.svelte';
  import Nav from './lib/components/shell/Nav.svelte';
  import ExpandedPlayer from './lib/components/player/ExpandedPlayer.svelte';
  import FloatingPlayer from './lib/components/player/FloatingPlayer.svelte';
  import NowPlayingBar from './lib/components/player/NowPlayingBar.svelte';
  import PairPage from './lib/components/pairing/PairPage.svelte';
  import PendingRequest from './lib/components/pairing/PendingRequest.svelte';

  const engine = new EngineClient();
  const position = new PlaybackPosition(engine);
  const library = new LibraryClient(engine);
  const layout = new LayoutStore();
  const themes = new UserThemes();
  const settings = new SettingsStore();
  const sheet = new PlayerSheet();
  const settingsView = new SettingsView();
  const views = new Views();
  setApp({ engine, library, position, layout, themes, settings, sheet, settingsView, views });
  // All Songs is the layout-arranged view; on desktop it also stands in for the Library home, which is the left panel.
  const allSongs = $derived(views.top.kind === 'songs' || (views.top.kind === 'home' && !views.phone));
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
    const accent = art?.accent ?? settings.current.accent;
    const root = document.documentElement;
    for (const [token, value] of Object.entries({ ...theme.tokens, ...art?.tokens })) root.style.setProperty(`--${token}`, value);
    root.style.colorScheme = art?.scheme ?? theme.scheme;
    setBarIconsDark(root.style.colorScheme === 'light');
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

  $effect(() => void applyDecorations(settings.current.ownDecorations));

  // Waveforms for the playing track and the next two, so the seek bar has them when they play (#16).
  $effect(() => {
    const current = engine.current;
    if (settings.current.seekBar !== 'waveform' || !current) return;
    const next = engine.player?.context?.next;
    const upcoming = [
      ...engine.queue,
      ...(next == null ? [] : engine.context.items.slice(Math.max(0, next - engine.context.offset))),
    ];
    // Untracked: the waveforms arriving mustn't rerun this.
    untrack(() => library.prefetchWaveforms([current, ...upcoming.slice(0, 2)].map((item) => item.track)));
  });

  // The playing track's art: 16px of pixels sampled from the art once
  // per track (and cached per art id), never while idle. It asks for the
  // 256px size the album grid already loaded, so it is usually a cache hit.
  let artPixels = $state<Uint8ClampedArray | null>(null);
  const pixelCache = new Map<string, Uint8ClampedArray | null>();
  // `undefined` while the track's details are looked up: keep the last colours rather than flashing the theme's.
  const artId = $derived.by(() => {
    if (settings.current.artColours === 'off' || !engine.current) return null;
    const details = library.details(engine.current.track);
    return details === undefined ? undefined : (details?.art ?? null);
  });
  $effect(() => {
    const id = artId;
    if (id === undefined) return;
    if (!id) {
      artPixels = null;
      return;
    }
    if (pixelCache.has(id)) {
      artPixels = pixelCache.get(id)!;
      return;
    }
    let stale = false;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
      const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true })!;
      ctx.canvas.width = ctx.canvas.height = 16;
      let pixels: Uint8ClampedArray | null = null;
      try {
        ctx.drawImage(img, 0, 0, 16, 16);
        pixels = ctx.getImageData(0, 0, 16, 16).data;
      } catch {
        // Tainted canvas (art served from another origin): keep the theme's colours.
      }
      pixelCache.set(id, pixels);
      if (!stale) artPixels = pixels;
    };
    img.onerror = () => !stale && (artPixels = null);
    img.src = library.artUrl(id, 256);
    return () => (stale = true);
  });
  // What the cover changes: accent only, or also the theme's backgrounds and text.
  const art = $derived.by(() => {
    const mode = settings.current.artColours;
    if (!artPixels || mode === 'off') return null;
    if (mode === 'accent') return { scheme: theme.scheme, tokens: {}, accent: accentFromPixels(artPixels, theme.tokens.bg) };
    return artPalette(artPixels, theme, mode);
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
    if (e.key === 'b' && (e.ctrlKey || e.metaKey) && !views.phone) {
      e.preventDefault();
      settings.set('navCollapsed', !settings.current.navCollapsed);
      return;
    }
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
    const devArt = params.get('dev-art');
    if (devArt) settings.current.artColours = devArt as typeof settings.current.artColours;
    if (params.get('dev-player') === 'floating') settings.current.player = 'floating';
    if (params.has('dev-sheet')) sheet.expand();
    if (params.has('dev-lyrics')) sheet.lyrics = true;
    const devView = devViews(params.get('dev-view') ?? '');
    if (devView) views.stack = devView;
    if (params.has('dev-arrange')) layout.arranging = true;
    const devSettings = params.get('dev-settings') as SettingsGroup;
    if (SETTINGS_GROUPS.includes(devSettings)) settingsView.open(devSettings);
    if (params.has('dev-pair')) settingsView.pairing = true;
    if (params.has('dev-add-engine')) settingsView.addingEngine = true;
    // A second engine to switch to, e.g. ?dev-engine=127.0.0.1:7711&dev-token=…
    const devEngine = params.get('dev-engine');
    if (devEngine) engines.update((list) => addEngine(list, { id: devEngine, name: devEngine, addresses: [devEngine], token: params.get('dev-token') }));
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

  const remote = engines.current.id !== LOCAL_ID;
  $effect(() => (remote ? routeVolumeKeys(engine) : undefined));
  // Apart from the updates below, so each change redraws the notification instead of hiding it first.
  const remoteControls = $derived(remote && settings.current.remoteControls);
  $effect(() => (remoteControls ? routeRemoteButtons(engine) : undefined));
  $effect(() => {
    if (!remoteControls) return;
    const art = library.details(engine.current?.track)?.art;
    const cover = art ? library.artUrl(art, 512, true) : null;
    showRemoteMedia(remoteMedia(engine.status, engine.player, position.at(Date.now()), cover));
  });

  // Opened from a host's pairing page ("Open in Motif", `MainActivity.kt`): pair as if its QR was scanned.
  const android = (globalThis as { MotifAndroid?: { takePairLink(): string | null } }).MotifAndroid;
  const takePairLink = () => {
    const text = android?.takePairLink();
    if (!text) return;
    const link = parsePairLink(text);
    if (link) settingsView.pairWith(link);
    else toasts.show("That isn't a Motif pairing link.");
  };
  (globalThis as { motifPairLinkArrived?: () => void }).motifPairLinkArrived = takePairLink;
  takePairLink();

  let editing = $state(false);

  // Android back closes the topmost layer. Menus and dialogs close through
  // their own Esc handling (the pair dialog also cancels its code there).
  const escape = () => document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  $effect(() =>
    routeBack(() => [
      { open: settingsView.scanning, close: () => void import('@tauri-apps/plugin-barcode-scanner').then((s) => s.cancel()) },
      { open: contextMenu.items.length > 0, close: escape },
      { open: helpOpen || settingsView.pairing || settingsView.addingEngine, close: escape },
      { open: editing, close: () => (editing = false) },
      { open: layout.arranging, close: () => (layout.arranging = false) },
      { open: !!settingsView.group, close: () => settingsView.close() },
      { open: sheet.open, close: () => sheet.collapse() },
      { open: !!views.parent, close: () => views.back() },
    ]),
  );
  const player = $derived(settings.current.player);
</script>

<svelte:document oncontextmenu={onContextMenu} />
<svelte:window onkeydown={onShortcut} ondragenter={onDragEnter} ondragleave={onDragLeave} ondragover={onDragOver} ondrop={onDrop} />

{#if engine.unpaired && !remote}
  <PairPage unpaired />
{:else}
<div class="shell">
  <Nav />

  <div class="body">
    {#if remote && (engine.unpaired || engine.status === 'closed')}
      <div class="error" role="alert">
        <span>{engine.unpaired ? `${engines.current.name} no longer accepts this device.` : `Can't reach ${engines.current.name}.`}</span>
        <button class="button" onclick={() => engines.update((list) => switchTo(list, LOCAL_ID))}>Switch to this device</button>
      </div>
    {/if}
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
      {:else if allSongs}
        {#key `${layout.source}/${layout.activeId}`}
          <ContentArea onEditLayout={() => (editing = true)} />
        {/key}
      {:else}
        {#key views.top}<ViewScreen />{/key}
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
<ResizeEdges />

{#if StressTest}<StressTest />{/if}

{#if editing}<LayoutEditor onClose={() => (editing = false)} />{/if}

{#if dragDepth > 0}
  <div class="drop-overlay" aria-hidden="true"><span>Drop a .kdl file to install it as a layout or theme</span></div>
{/if}
{/if}

<div class="toasts" role="status">
  <!-- Every Full client is asked, not only the one that showed the code. -->
  {#if !settingsView.pairing}
    {#each engine.pairing?.pending ?? [] as request (request.id)}
      <div class="toast"><PendingRequest {request} /></div>
    {/each}
  {/if}
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
    padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left);
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
    bottom: calc(var(--bar-height) + env(safe-area-inset-bottom) + 16px);
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
