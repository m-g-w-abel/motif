<script lang="ts">
  import { getApp } from '../../context';
  import { addFolder } from '../../folders';
  import { installTheme } from '../../layout/install';
  import { moveItem } from '../../reorder';
  import type { Settings } from '../../settings.svelte';
  import type { SettingsGroup } from '../../settingsView.svelte';
  import { SYSTEM_THEMES, THEMES, themeById } from '../../themes';
  import CopyButton from '../CopyButton.svelte';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ShortcutList from './ShortcutList.svelte';
  import Slider from '../Slider.svelte';
  import { FONT_LIST } from '../../themeFile';
  import { canDecorate } from '../../windowChrome';
  import WindowButtons from './WindowButtons.svelte';

  const { settings, themes, engine, library, settingsView } = getApp();
  let panel = $state<HTMLElement>();

  const groups: { id: SettingsGroup; label: string }[] = [
    { id: 'interface', label: 'Interface' },
    { id: 'library', label: 'Library' },
    { id: 'playback', label: 'Playback' },
    { id: 'keyboard', label: 'Keyboard' },
    { id: 'integrations', label: 'Integrations' },
    { id: 'technical', label: 'Technical' },
    { id: 'about', label: 'About' },
  ];
  const group = $derived(settingsView.group);

  // Devices come and go; refresh the list whenever Playback shows.
  $effect(() => {
    if (group === 'playback' && engine.status === 'open') engine.run({ type: 'list_outputs' });
  });

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

  const enabledDecoders = $derived(decoders.filter((d) => d.on));

  function setDecoders(order: string[]) {
    engine.run({ type: 'set_decoders', order });
  }

  function toggleDecoder(name: string, on: boolean) {
    const order = engine.player?.decoders ?? [];
    setDecoders(on ? [...order, name] : order.filter((d) => d !== name));
  }

  function query(q: Parameters<typeof engine.query>[0]) {
    engine.query(q).catch((e: Error) => (engine.error = e.message));
  }

  const accents = ['#e8a33d', '#a47cf3', '#4fa3ff', '#4fb3a9', '#ff5c8a', '#7ac74f'];

  type Choice<K extends keyof Settings> = { key: K; label: string; options: [Settings[K], string][] };
  const choices = [
    { key: 'density', label: 'Density', options: [['compact', 'Compact'], ['comfortable', 'Comfortable']] },
    { key: 'player', label: 'Player', options: [['bar', 'Bar'], ['floating', 'Floating']] },
    { key: 'seekBar', label: 'Seek bar', options: [['waveform', 'Waveform'], ['plain', 'Plain']] },
  ] as [Choice<'density'>, Choice<'player'>, Choice<'seekBar'>];

  const snapScale = (v: number) => 90 + Math.round(v / 5) * 5;
  // The number follows the thumb; the layout is only redone on release.
  let scaleDraft = $state<number>();
  const scaleShown = $derived(scaleDraft ?? settings.current.scale);
  const OTHER = '\0other';
  const FONTS = [['', 'Theme default'], ['system-ui', 'System UI'], ['sans-serif', 'Sans-serif'], ['serif', 'Serif'], ['monospace', 'Monospace'], ['Inter', 'Inter']];
  let fontOther = $state(!FONTS.some(([v]) => v === settings.current.font));

  let themeInput = $state<HTMLInputElement>();
  const activeUserTheme = $derived(themes.byId(settings.current.theme));

  async function pickThemes(files: FileList | null) {
    for (const file of files ?? []) {
      const id = installTheme(themes, await file.text(), file.name);
      if (id) settings.set('theme', id);
    }
    if (themeInput) themeInput.value = '';
  }

  const themeOptions = $derived([
    ...[...THEMES, ...themes.list].map((t) => ({ id: t.id, name: t.name, preview: t, split: null })),
    // "System" previews both of its themes, half and half.
    {
      id: 'system',
      name: 'System',
      preview: themeById(SYSTEM_THEMES.light),
      split: themeById(SYSTEM_THEMES.dark),
    },
  ]);
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && settingsView.close()} />

<!--
  To add a setting: put a `<div class="row">` with a `<span class="label">` and
  its control in the right group's `{#if}` below.
-->
<section class="screen" aria-label="Settings">
  <header {...settings.current.ownDecorations && canDecorate ? { 'data-tauri-drag-region': '' } : {}}>
    <button class="button" onclick={() => settingsView.close()}>← Back</button>
    <h1 data-tauri-drag-region={settings.current.ownDecorations && canDecorate ? '' : undefined}>Settings</h1>
    <WindowButtons />
  </header>

  <div class="layout">
    <nav class="groups" aria-label="Settings groups">
      {#each groups as g (g.id)}
        <button class:active={group === g.id} aria-current={group === g.id ? 'page' : undefined} onclick={() => settingsView.open(g.id)}>
          {g.label}
        </button>
      {/each}
    </nav>

    <div class="panel" bind:this={panel}>
      {#if group === 'interface'}
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
          <div class="actions">
            <button class="button" onclick={() => themeInput?.click()}>Install theme…</button>
            {#if activeUserTheme}
              <button
                class="button"
                onclick={() => {
                  themes.remove(activeUserTheme.id);
                  settings.set('theme', 'graphite');
                }}
              >
                Remove “{activeUserTheme.name}”
              </button>
            {/if}
            <input bind:this={themeInput} type="file" accept=".kdl" multiple hidden onchange={(e) => pickThemes(e.currentTarget.files)} />
          </div>
          <p class="status">Or drop a theme .kdl file onto the window.</p>
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
          <span class="label">Scale</span>
          <div class="scale">
            <Slider
              label="Interface scale"
              value={settings.current.scale - 90}
              max={60}
              valueText="{scaleShown}%"
              oninput={(v) => (scaleDraft = snapScale(v))}
              onchange={(v) => {
                settings.set('scale', snapScale(v));
                scaleDraft = undefined;
              }}
            />
            <span class="status">{scaleShown}%</span>
          </div>
        </div>

        <div class="row">
          <span class="label">Font</span>
          <select
            class="select"
            aria-label="Font family"
            value={fontOther ? OTHER : settings.current.font}
            onchange={(e) => {
              fontOther = e.currentTarget.value === OTHER;
              if (!fontOther) settings.set('font', e.currentTarget.value);
            }}
          >
            {#each FONTS as [value, text] (value)}<option {value}>{text}</option>{/each}
            <option value={OTHER}>Other…</option>
          </select>
          {#if fontOther}
            <input
              class="select"
              placeholder="Installed font family"
              aria-label="Other font family"
              value={settings.current.font}
              onchange={(e) => settings.set('font', FONT_LIST.test(e.currentTarget.value) ? e.currentTarget.value : '')}
            />
          {/if}
        </div>

        {#if canDecorate}
          <div class="row">
            <span class="label">Window buttons in the app</span>
            <div class="segmented" role="radiogroup" aria-label="Window buttons in the app">
              {#each [[false, 'Off'], [true, 'On']] as [value, text] (text)}
                <button
                  role="radio"
                  aria-checked={settings.current.ownDecorations === value}
                  class:active={settings.current.ownDecorations === value}
                  onclick={() => settings.set('ownDecorations', value as boolean)}
                >
                  {text}
                </button>
              {/each}
            </div>
            <p class="status">Replaces the system title bar; takes effect at once.</p>
          </div>
        {/if}

        <div class="row">
          <span class="label">Colours from artwork</span>
          <div class="segmented" role="radiogroup" aria-label="Colours from artwork">
            {#each [['off', 'Off'], ['accent', 'Accent'], ['tint', 'Tinted theme'], ['full', 'Full theme']] as const as [value, text] (value)}
              <button role="radio" aria-checked={settings.current.artColours === value} class:active={settings.current.artColours === value} onclick={() => settings.set('artColours', value)}>{text}</button>
            {/each}
          </div>
        </div>

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
      {:else if group === 'library'}
        {#if library.status}
          {@const status = library.status}
          <div class="row">
            <span class="label">Music folders</span>
            {#each status.roots as root (root)}
              <div class="item">
                <span class="path selectable" title={root}>{root}</span>
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
              {#if status.missing}{' · '}{status.missing} missing{/if}
              {#if status.scanning}{' · '}scanning ({status.scanned} files){/if}
              {#if status.hashing}{' · '}hashing {status.hashing} files{/if}
            </p>
            {#if status.error}<p class="status error selectable">{status.error} <CopyButton text={status.error} /></p>{/if}
            <label class="decoder">
              <input
                type="checkbox"
                checked={status.watching}
                onchange={(e) => query({ type: 'set_watching', enabled: e.currentTarget.checked })}
              />
              Pick up changes made outside Motif
            </label>
          </div>
        {:else}
          <p class="status">The library isn't available yet.</p>
        {/if}
      {:else if group === 'playback'}
        {#if engine.player}
          {@const player = engine.player}
          <div class="row">
            <span class="label">Output</span>
            <select
              class="select"
              aria-label="Output device"
              value={player.output ?? ''}
              onchange={(e) => engine.run({ type: 'set_output', device: e.currentTarget.value || null })}
            >
              <option value="">System default</option>
              {#each player.outputs as device (device.id)}
                <option value={device.id}>{device.name}</option>
              {/each}
              {#if player.output && !player.outputs.some((d) => d.id === player.output)}
                <option value={player.output}>{player.output} (not found)</option>
              {/if}
            </select>
          </div>
        {/if}

        {#if decoders.length}
          <div class="row">
            <span class="label">Decoders</span>
            <ReorderList
              items={enabledDecoders}
              label="Decoders"
              key={(d) => d.name}
              onmove={(from, to) => setDecoders(moveItem(enabledDecoders, from, to).map((d) => d.name))}
            >
              {#snippet row(decoder)}
                {@render decoderLabel(decoder)}
              {/snippet}
            </ReorderList>
            {#each decoders.filter((d) => !d.on) as decoder (decoder.name)}
              <div class="item off">{@render decoderLabel(decoder)}</div>
            {/each}
            <p class="status">Each file plays with the first of these that can open it.</p>
          </div>
        {:else if !engine.player}
          <p class="status">Not connected to the engine.</p>
        {/if}
      {:else if group === 'keyboard'}
        <div class="row">
          <span class="label">Vim keys (navigation only)</span>
          <div class="segmented" role="radiogroup" aria-label="Vim keys">
            {#each [[false, 'Off'], [true, 'On']] as [value, text] (text)}
              <button
                role="radio"
                aria-checked={settings.current.vimKeys === value}
                class:active={settings.current.vimKeys === value}
                onclick={() => settings.set('vimKeys', value as boolean)}
              >
                {text}
              </button>
            {/each}
          </div>
        </div>
        <ShortcutList />
      {:else if group === 'integrations'}
        {#if engine.integrations}
          {@const integrations = engine.integrations}
          <div class="row">
            <span class="label">Lyrics from LRCLIB</span>
            <div class="segmented" role="radiogroup" aria-label="Lyrics from LRCLIB">
              {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                <button role="radio" aria-checked={integrations.lrclib === value} class:active={integrations.lrclib === value} onclick={() => engine.setIntegrations({ ...integrations, lrclib: value })}>{text}</button>
              {/each}
            </div>
            <p class="status">Looks up lyrics for what plays on lrclib.net. Applies to every window and device using this engine.</p>
          </div>
          {#if integrations.secrets_unencrypted}
            <p class="status">No system keyring found: sign-ins are stored unencrypted, readable only by your user.</p>
          {/if}
        {:else}
          <p class="status">Integration settings aren't available yet.</p>
        {/if}
      {:else if group === 'technical'}
        <div class="row">
          <span class="label">Engine connection</span>
          <p class="status">{engine.status}</p>
        </div>
      {:else if group === 'about'}
        <div class="row">
          <span class="label">Motif</span>
          <p class="status">A music player for collectors.</p>
        </div>
      {/if}
    </div>
    <Scrollbar target={panel} />
  </div>
</section>

{#snippet decoderLabel(decoder: { name: string; on: boolean })}
  <label class="decoder">
    <input type="checkbox" checked={decoder.on} onchange={(e) => toggleDecoder(decoder.name, e.currentTarget.checked)} />
    {decoderLabels[decoder.name] ?? decoder.name}
  </label>
{/snippet}

<style>
  .screen {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--bg);
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border-bottom: 1px solid var(--divider);
    flex: none;
  }

  h1 {
    margin: 0;
    font-size: 16px;
  }

  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .groups {
    display: flex;
    flex-direction: column;
    width: 180px;
    padding: var(--space-2);
    border-right: 1px solid var(--divider);
    flex: none;
  }

  .groups button {
    height: 32px;
    padding: 0 var(--space-3);
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .groups button:hover {
    background: var(--hover);
  }

  .groups button.active {
    background: var(--hover);
    color: var(--accent);
    font-weight: 600;
  }

  .panel {
    flex: 1;
    min-width: 0;
    max-width: 640px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
    overflow-y: auto;
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

  .off {
    padding-left: 22px;
  }

  .icon-button.small {
    width: 22px;
    height: 22px;
  }

  .select {
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: 12px;
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
    align-self: flex-start;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .segmented button {
    height: 26px;
    padding: 0 var(--space-3);
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
    grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
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

  .scale {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 220px;
  }

  .scale :global(.slider) {
    flex: 1;
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
    /* Full screen, over the player bar too. */
    .screen {
      position: fixed;
      inset: 0;
      z-index: 30;
    }

    .layout {
      flex-direction: column;
    }

    .groups {
      flex-direction: row;
      width: auto;
      overflow-x: auto;
      border-right: 0;
      border-bottom: 1px solid var(--divider);
    }

    .groups button {
      flex: none;
    }
  }
</style>
