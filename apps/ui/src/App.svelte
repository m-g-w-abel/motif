<script lang="ts">
  import { onDestroy } from 'svelte';
  import { setApp } from './lib/context';
  import { demoLibrary } from './lib/demo';
  import { Drawer } from './lib/drawer.svelte';
  import { EngineClient, PlaybackPosition } from './lib/engine.svelte';
  import { installLayoutFiles } from './lib/layout/install';
  import { LayoutStore } from './lib/layout/store.svelte';
  import { SettingsStore } from './lib/settings.svelte';
  import { SYSTEM_THEMES, themeById } from './lib/themes';
  import { MediaQuery } from 'svelte/reactivity';
  import { PlayerSheet } from './lib/sheet.svelte';
  import { toasts } from './lib/toast.svelte';
  import LayoutEditor from './lib/components/shell/LayoutEditor.svelte';
  import ContentArea from './lib/components/shell/ContentArea.svelte';
  import Nav from './lib/components/shell/Nav.svelte';
  import ExpandedPlayer from './lib/components/player/ExpandedPlayer.svelte';
  import FloatingPlayer from './lib/components/player/FloatingPlayer.svelte';
  import NowPlayingBar from './lib/components/player/NowPlayingBar.svelte';

  const engine = new EngineClient();
  const position = new PlaybackPosition(engine);
  const layout = new LayoutStore();
  const settings = new SettingsStore();
  const sheet = new PlayerSheet();
  const drawer = new Drawer();
  setApp({ engine, position, layout, settings, sheet, drawer });

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
  const theme = $derived(
    themeById(
      settings.current.theme === 'system'
        ? SYSTEM_THEMES[prefersDark.current ? 'dark' : 'light']
        : settings.current.theme,
    ),
  );
  $effect(() => {
    const { accent, density } = settings.current;
    const root = document.documentElement;
    for (const [token, value] of Object.entries(theme.tokens)) root.style.setProperty(`--${token}`, value);
    root.style.colorScheme = theme.scheme;
    root.dataset.density = density;
    root.style.setProperty('--accent', accent);
    root.style.setProperty('--accent-contrast', contrastFor(accent));
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

  // The app has its own interactions; the browser's context menu ("Inspect",
  // "Reload"…) only makes sense in text fields.
  function onContextMenu(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest('input, textarea, [contenteditable="true"]')) e.preventDefault();
  }

  // Dropping .kdl files anywhere installs them as layouts.
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
    void installLayoutFiles(layout, e.dataTransfer?.files ?? []);
  }

  // Development only (stripped from builds): URL parameters that set up a
  // state for visual checks in other webviews, e.g.
  // ?dev-layout=builtin:details.kdl&dev-theme=light&dev-play=5&dev-sheet=1
  if (import.meta.env.DEV) {
    const params = new URLSearchParams(location.search);
    const devLayout = params.get('dev-layout');
    if (devLayout) layout.select(devLayout);
    const devTheme = params.get('dev-theme');
    if (devTheme) settings.current.theme = devTheme;
    if (params.get('dev-player') === 'floating') settings.current.player = 'floating';
    if (params.has('dev-sheet')) sheet.expand();
    if (params.has('dev-drawer')) drawer.open = true;
    const devPlay = params.get('dev-play');
    if (devPlay !== null) {
      $effect(() => {
        if (engine.status === 'open') {
          engine.run({ type: 'set_queue', tracks: demoLibrary, start: Number(devPlay) });
        }
      });
    }
  }

  let editing = $state(false);
  const player = $derived(settings.current.player);
</script>

<svelte:document oncontextmenu={onContextMenu} />
<svelte:window onpointerdown={onPointerDown} ondragenter={onDragEnter} ondragleave={onDragLeave} ondragover={onDragOver} ondrop={onDrop} />

<div class="shell">
  <Nav />

  <div class="body">
    {#if engine.error}
      <div class="error" role="alert">
        <span>{engine.error}</span>
        <button class="icon-button" aria-label="Dismiss" onclick={() => (engine.error = null)}>×</button>
      </div>
    {/if}

    <main>
      {#key `${layout.source}/${layout.activeId}`}
        <ContentArea onEditLayout={() => (editing = true)} />
      {/key}
    </main>

    {#if player === 'bar'}<NowPlayingBar />{/if}

    <ExpandedPlayer />
  </div>

  {#if player === 'floating' && !sheet.open}<FloatingPlayer corner={settings.current.floatingCorner} />{/if}
</div>

{#if editing}<LayoutEditor onClose={() => (editing = false)} />{/if}

{#if dragDepth > 0}
  <div class="drop-overlay" aria-hidden="true"><span>Drop a .kdl file to install it as a layout</span></div>
{/if}

<div class="toasts" role="status">
  {#each toasts.items as toast (toast.id)}
    <button class="toast {toast.kind}" onclick={() => toasts.dismiss(toast.id)}>{toast.message}</button>
  {/each}
</div>

<style>
  .shell {
    display: flex;
    height: 100vh;
    height: 100dvh;
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
    max-width: min(520px, calc(100vw - 32px));
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: left;
    cursor: pointer;
    pointer-events: auto;
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
