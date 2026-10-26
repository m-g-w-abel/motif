<script lang="ts">
  import { getApp } from '../../context';
  import { addFolder } from '../../folders';
  import type { Settings } from '../../settings.svelte';
  import { SYSTEM_THEMES, THEMES, themeById } from '../../themes';
  import Icon from '../Icon.svelte';

  const { settings, engine, library } = getApp();

  const decoderLabels: Record<string, string> = {
    libopus: 'libopus · Opus',
    symphonia: 'Symphonia · FLAC, MP3, AAC, Vorbis, WAV',
    ffmpeg: 'FFmpeg · everything else',
  };
  /** Enabled decoders in order, then the disabled ones. */
  const decoders = $derived.by(() => {
    const player = engine.player;
    if (!player) return [];
    const disabled = player.available_decoders.filter((d) => !player.decoders.includes(d));
    return [...player.decoders.map((name) => ({ name, on: true })), ...disabled.map((name) => ({ name, on: false }))];
  });

  function setDecoders(order: string[]) {
    engine.run({ type: 'set_decoders', order });
  }

  function moveDecoder(name: string, by: -1 | 1) {
    const order = [...(engine.player?.decoders ?? [])];
    const i = order.indexOf(name);
    if (i < 0 || i + by < 0 || i + by >= order.length) return;
    [order[i], order[i + by]] = [order[i + by], order[i]];
    setDecoders(order);
  }

  function toggleDecoder(name: string, on: boolean) {
    const order = engine.player?.decoders ?? [];
    setDecoders(on ? [...order, name] : order.filter((d) => d !== name));
  }

  function query(q: Parameters<typeof engine.query>[0]) {
    engine.query(q).catch((e: Error) => (engine.error = e.message));
  }
  let open = $state(false);
  let root = $state<HTMLElement>();

  const accents = ['#e8a33d', '#a47cf3', '#4fa3ff', '#4fb3a9', '#ff5c8a', '#7ac74f'];

  type Choice<K extends keyof Settings> = { key: K; label: string; options: [Settings[K], string][] };
  const choices = [
    { key: 'density', label: 'Density', options: [['compact', 'Compact'], ['comfortable', 'Comfortable']] },
    { key: 'player', label: 'Player', options: [['bar', 'Bar'], ['floating', 'Floating']] },
  ] as [Choice<'density'>, Choice<'player'>];

  const themeOptions = [
    ...THEMES.map((t) => ({ id: t.id, name: t.name, preview: t, split: null })),
    // "System" previews both of its themes, half and half.
    {
      id: 'system',
      name: 'System',
      preview: themeById(SYSTEM_THEMES.light),
      split: themeById(SYSTEM_THEMES.dark),
    },
  ];

  function onWindowPointer(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdown={onWindowPointer} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="settings" bind:this={root}>
  <button
    class="icon-button"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label="Settings"
    title="Settings"
    onclick={() => (open = !open)}
  >
    <Icon name="settings" size={16} />
  </button>

  {#if open}
    <div class="popover" role="dialog" aria-label="Settings">
      <div class="row">
        <span class="label">Theme</span>
        <div class="themes" role="radiogroup" aria-label="Theme">
          {#each themeOptions as option (option.id)}
            <button
              class="theme"
              class:active={settings.current.theme === option.id}
              role="radio"
              aria-checked={settings.current.theme === option.id}
              onclick={() => settings.set('theme', option.id)}
            >
              <span
                class="preview"
                style:background={option.split
                  ? `linear-gradient(135deg, ${option.preview.tokens.bg} 50%, ${option.split.tokens.bg} 50%)`
                  : option.preview.tokens.bg}
              >
                <span class="bar" style:background={option.preview.tokens.surface}></span>
                <span class="line" style:background={option.preview.tokens.text}></span>
                <span class="line short" style:background={settings.current.accent}></span>
              </span>
              <span class="name">{option.name}</span>
            </button>
          {/each}
        </div>
      </div>

      {#each choices as choice (choice.key)}
        <div class="row">
          <span class="label">{choice.label}</span>
          <div class="segmented" role="radiogroup" aria-label={choice.label}>
            {#each choice.options as [value, text] (value)}
              <button
                role="radio"
                aria-checked={settings.current[choice.key] === value}
                class:active={settings.current[choice.key] === value}
                onclick={() => settings.set(choice.key, value as never)}
              >
                {text}
              </button>
            {/each}
          </div>
        </div>
      {/each}

      <div class="row">
        <span class="label">Accent</span>
        <div class="swatches">
          {#each accents as color (color)}
            <button
              class="swatch"
              class:active={settings.current.accent === color}
              style:background={color}
              aria-label="Accent {color}"
              onclick={() => settings.set('accent', color)}
            ></button>
          {/each}
          <label class="swatch custom" title="Custom colour">
            <input
              type="color"
              value={settings.current.accent}
              aria-label="Custom accent colour"
              oninput={(e) => settings.set('accent', e.currentTarget.value)}
            />
          </label>
        </div>
      </div>

      {#if library.status}
        {@const status = library.status}
        <div class="row">
          <span class="label">Music folders</span>
          {#each status.roots as root (root)}
            <div class="item">
              <span class="path" title={root}>{root}</span>
              <button class="icon-button small" aria-label="Remove {root}" title="Remove from the library (files stay)" onclick={() => query({ type: 'remove_root', path: root })}>
                <Icon name="close" size={12} />
              </button>
            </div>
          {/each}
          <div class="actions">
            <button class="button" onclick={() => addFolder(engine)}>Add folder…</button>
            <button class="button" disabled={status.scanning || status.roots.length === 0} onclick={() => query({ type: 'rescan' })}>Rescan</button>
          </div>
          <p class="status">
            {status.tracks} tracks
            {#if status.scanning}{' · '}scanning ({status.scanned} files){/if}
            {#if status.hashing}{' · '}hashing {status.hashing} files{/if}
          </p>
          {#if status.error}<p class="status error">{status.error}</p>{/if}
        </div>
      {/if}

      {#if decoders.length}
        <div class="row">
          <span class="label">Decoders</span>
          {#each decoders as decoder, i (decoder.name)}
            <div class="item">
              <label class="decoder">
                <input type="checkbox" checked={decoder.on} onchange={(e) => toggleDecoder(decoder.name, e.currentTarget.checked)} />
                {decoderLabels[decoder.name] ?? decoder.name}
              </label>
              {#if decoder.on}
                <button class="icon-button small" aria-label="Try {decoder.name} earlier" disabled={i === 0} onclick={() => moveDecoder(decoder.name, -1)}>
                  <Icon name="chevron-up" size={12} />
                </button>
                <button class="icon-button small" aria-label="Try {decoder.name} later" disabled={!decoders[i + 1]?.on} onclick={() => moveDecoder(decoder.name, 1)}>
                  <Icon name="chevron-down" size={12} />
                </button>
              {/if}
            </div>
          {/each}
          <p class="status">Each file plays with the first of these that can open it.</p>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .settings {
    position: relative;
  }

  .popover {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    width: 290px;
    max-height: calc(100vh - 80px);
    overflow-y: auto;
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .row {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-height: 24px;
  }

  .path,
  .decoder {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }

  .decoder {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    cursor: pointer;
  }

  .icon-button.small {
    width: 22px;
    height: 22px;
  }

  .actions {
    display: flex;
    gap: var(--space-2);
  }

  .status {
    margin: 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .status.error {
    color: var(--danger);
  }

  .label {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .segmented {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .segmented button {
    flex: 1;
    height: 26px;
    border: 0;
    border-right: 1px solid var(--border);
    background: none;
    font-size: 12px;
    cursor: pointer;
  }

  .segmented button:last-child {
    border-right: 0;
  }

  .segmented button:hover {
    background: var(--hover);
  }

  .segmented button.active {
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
  }

  .themes {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--space-2);
  }

  .theme {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }

  .preview {
    display: flex;
    flex-direction: column;
    gap: 4px;
    height: 40px;
    padding: 0 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .theme.active .preview {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .preview .bar {
    height: 9px;
    flex: none;
  }

  .preview .line {
    height: 3px;
    width: 70%;
    margin-left: 6px;
    border-radius: 1px;
    opacity: 0.8;
  }

  .preview .line.short {
    width: 40%;
    opacity: 1;
  }

  .theme .name {
    font-size: 11px;
    color: var(--text-muted);
  }

  .theme.active .name {
    color: var(--text);
    font-weight: 600;
  }

  .swatches {
    display: flex;
    gap: var(--space-2);
  }

  .swatch {
    position: relative;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }

  .swatch.active {
    outline: 2px solid var(--text);
    outline-offset: 1px;
  }

  .custom {
    background: conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
    overflow: hidden;
  }

  .custom input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  @media (max-width: 700px) {
    .popover {
      position: fixed;
      left: var(--space-2);
      right: var(--space-2);
      bottom: 64px;
      width: auto;
    }
  }
</style>
