<script lang="ts">
  import { PRESETS, presetOf } from '../../access';
  import { getApp } from '../../context';
  import type { Preset } from '../../protocol/Preset';
  import type { Scope } from '../../protocol/Scope';
  import AddEngineDialog from '../pairing/AddEngineDialog.svelte';
  import PairDialog from '../pairing/PairDialog.svelte';
  import { addFolder, androidStorage } from '../../folders';
  import { installTheme } from '../../layout/install';
  import { openUrl } from '../../openUrl';
  import { moveItem } from '../../reorder';
  import type { ReplayGainMode } from '../../protocol/ReplayGainMode';
  import type { ReplayGainSettings } from '../../protocol/ReplayGainSettings';
  import type { Settings } from '../../settings.svelte';
  import type { SettingsGroup } from '../../settingsView.svelte';
  import { SYSTEM_THEMES, THEMES, themeById } from '../../themes';
  import CopyButton from '../CopyButton.svelte';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ShortcutList from './ShortcutList.svelte';
  import Slider from '../Slider.svelte';
  import { CHOICES, mapped, type Action, type Gesture, type Surface } from '../../gestures';
  import { FONT_LIST } from '../../themeFile';
  import { kdlAccept, isAndroid } from '../../platform';
  import { canDecorate } from '../../windowChrome';
  import { canDiscover } from '../../discover';
  import { canShowRemoteMedia } from '../../remoteMedia';
  import { engines } from '../../engine.svelte';
  import { LOCAL_ID, removeEngine, switchTo } from '../../engines';
  import WindowButtons from './WindowButtons.svelte';

  const { settings, themes, engine, library, settingsView } = getApp();
  let panel = $state<HTMLElement>();

  const groups: { id: SettingsGroup; label: string }[] = [
    { id: 'interface', label: 'Interface' },
    { id: 'library', label: 'Library' },
    { id: 'playback', label: 'Playback' },
    { id: 'keyboard', label: 'Keyboard' },
    { id: 'gestures', label: 'Gestures' },
    { id: 'integrations', label: 'Integrations' },
    { id: 'devices', label: 'Devices' },
    { id: 'technical', label: 'Technical' },
    { id: 'about', label: 'About' },
  ];
  const group = $derived(settingsView.group);
  /** What a group's controls need; other devices see that they can't change it. */
  const needs: Partial<Record<SettingsGroup, Scope>> = { library: 'library', playback: 'settings', integrations: 'settings', devices: 'pairing' };
  const allowed = $derived(!group || !needs[group] || engine.can(needs[group]));

  const LOG_ACTIONS: Record<string, string> = {
    paired: 'paired automatically',
    approved: 'approved',
    denied: 'denied',
    revoked: 'removed',
    renamed: 'renamed',
    preset_changed: 'access changed',
  };
  /** The device whose Remove waits for a second click. */
  let removing = $state<string | null>(null);
  const engineTime = (ms: number) =>
    new Date(ms - engine.clockOffset).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
  function rename(device: string, current: string, input: HTMLInputElement) {
    const name = input.value.trim();
    if (!name) input.value = current;
    else if (name !== current) engine.pair({ type: 'rename', device, name });
  }

  /** Android only: rechecked when the app comes back from the system toggle. */
  const storage = androidStorage();
  let allFilesAccess = $state(storage?.hasAllFilesAccess() ?? true);

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

  const REPLAY_GAIN_MODES: [ReplayGainMode, string][] = [
    ['off', 'Off'],
    ['track', 'Track'],
    ['album', 'Album'],
    ['auto', 'Auto'],
  ];
  /** The ReplayGain sliders go this many dB either way. */
  const GAIN_RANGE = 15;
  const formatDb = (db: number) => `${db > 0 ? '+' : ''}${db} dB`;

  function setReplayGain(change: Partial<ReplayGainSettings>) {
    if (engine.player) engine.run({ type: 'set_replay_gain', settings: { ...engine.player.replay_gain, ...change } });
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

  const SURFACES: Record<Surface, string> = { bar: 'the player bar', artwork: 'the artwork', row: 'a track or album' };
  const GESTURE_NAMES: Record<Gesture, string> = {
    'swipe-left': 'Swipe left on',
    'swipe-right': 'Swipe right on',
    'drag-vertical': 'Drag up or down on',
    'long-press': 'Long-press',
  };
  const ACTIONS: Record<Action | 'none', string> = {
    next: 'Next track',
    previous: 'Previous track',
    queue: 'Add to queue',
    'play-next': 'Queue next',
    volume: 'Volume',
    seek: 'Seek',
    none: 'None',
  };
  const gestureRows = (Object.keys(CHOICES) as Surface[]).flatMap((surface) =>
    (Object.entries(CHOICES[surface]) as [Gesture, Action[]][]).map(([gesture, options]) => ({
      surface,
      gesture,
      options: [...options, 'none'] as const,
    })),
  );

  const snapScale = (v: number) => 90 + Math.round(v / 5) * 5;
  // The number follows the thumb; the layout is only redone on release.
  let scaleDraft = $state<number>();
  const scaleShown = $derived(scaleDraft ?? settings.current.scale);
  const OTHER = '\0other';
  const FONTS = [['', 'Theme default'], ['system-ui', 'System UI'], ['sans-serif', 'Sans-serif'], ['serif', 'Serif'], ['monospace', 'Monospace'], ['Inter', 'Inter']];
  let fontOther = $state(!FONTS.some(([v]) => v === settings.current.font));

  let themeInput = $state<HTMLInputElement>();
  let listenBrainzToken = $state('');
  let mpdPassword = $state('');
  /** Signs the host in to a scrobbling service (a ListenBrainz user token; anything for Last.fm and Libre.fm), or out with ''. */
  function signIn(service: 'listenbrainz' | 'lastfm' | 'librefm', credential: string) {
    const account = engine.integrations?.[service];
    if (account) engine.setIntegrations({ [service]: { ...account, sign_in: credential } });
  }
  // Opens a service's sign-in page once the host has one, in the window that asked only.
  let browserSignIn = $state<'lastfm' | 'librefm' | null>(null);
  $effect(() => {
    const url = browserSignIn && engine.integrations?.[browserSignIn].auth_url;
    if (url) {
      browserSignIn = null;
      void openUrl(url);
    }
  });
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
<svelte:document onvisibilitychange={() => (allFilesAccess = storage?.hasAllFilesAccess() ?? true)} />

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
      {#if group === 'devices' && engines.list.engines.length > 1}
        <div class="row">
          <span class="label">Engine in control</span>
          {#each engines.list.engines as saved (saved.id)}
            <div class="device">
              <span class="name">{saved.name}</span>
              {#if saved.id === engines.list.current}
                <span class="status">{engine.unpaired ? 'Not paired' : { connecting: 'Connecting…', open: '● Connected', closed: 'Disconnected' }[engine.status]}</span>
              {:else}
                <button class="button" onclick={() => engines.update((list) => switchTo(list, saved.id))}>Switch</button>
              {/if}
              {#if saved.id !== LOCAL_ID}
                <button class="button" onclick={() => engines.update((list) => removeEngine(list, saved.id))}>Forget</button>
              {/if}
            </div>
          {/each}
          <p class="status">Controls one engine at a time. Music playing on this device keeps playing while you control another.</p>
        </div>
      {/if}
      {#if group === 'devices' && canDiscover()}
        <div class="row">
          <span class="label">Other engines</span>
          <p class="status">Control Motif running on another computer on this network.</p>
          <div class="actions">
            <button class="button" onclick={() => (settingsView.addingEngine = true)}>Add an engine…</button>
          </div>
        </div>
      {/if}
      {#if group === 'devices' && canShowRemoteMedia}
        <div class="row">
          <span class="label">Remote playback controls</span>
          <div class="segmented" role="radiogroup" aria-label="Remote playback controls">
            {#each [[false, 'Off'], [true, 'On']] as [value, text] (text)}
              <button
                role="radio"
                aria-checked={settings.current.remoteControls === value}
                class:active={settings.current.remoteControls === value}
                onclick={() => settings.set('remoteControls', value as boolean)}
              >
                {text}
              </button>
            {/each}
          </div>
          <p class="status">While you control another engine, its track and buttons show in the notification and on the lock screen.</p>
        </div>
      {/if}
      {#if !allowed}
        <p class="status">This device can't change these settings. The host can give it Full access under Settings → Devices.</p>
      {:else if group === 'interface'}
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
            <input bind:this={themeInput} type="file" accept={kdlAccept} multiple hidden onchange={(e) => pickThemes(e.currentTarget.files)} />
          </div>
          {#if !isAndroid}<p class="status">Or drop a theme .kdl file onto the window.</p>{/if}
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
        {#if !allFilesAccess && storage}
          <div class="row">
            <span class="label">All files access</span>
            <p class="status">
              Motif plays music straight from the folders you add, with their cover images and lyrics files. Android only
              allows that with All files access.
            </p>
            <div class="actions">
              <button class="button primary" onclick={() => storage.openAllFilesSettings()}>Allow All files access…</button>
            </div>
          </div>
        {/if}
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

          {@const rg = player.replay_gain}
          <div class="row">
            <span class="label">ReplayGain</span>
            <div class="segmented" role="radiogroup" aria-label="ReplayGain">
              {#each REPLAY_GAIN_MODES as [mode, text] (mode)}
                <button
                  role="radio"
                  aria-checked={rg.mode === mode}
                  class:active={rg.mode === mode}
                  onclick={() => setReplayGain({ mode })}
                >
                  {text}
                </button>
              {/each}
            </div>
            <p class="status">Auto uses album gain when playing an album, track gain otherwise.</p>
          </div>
          {#if rg.mode !== 'off'}
            {@render gainSlider('Preamp', 'preamp_db', rg.preamp_db)}
            {@render gainSlider('Files without gain tags', 'untagged_db', rg.untagged_db)}
            <div class="row">
              <label class="decoder">
                <input
                  type="checkbox"
                  checked={rg.prevent_clipping}
                  onchange={(e) => setReplayGain({ prevent_clipping: e.currentTarget.checked })}
                />
                Prevent clipping (lower the gain to keep the tagged peak)
              </label>
            </div>
          {/if}
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
      {:else if group === 'gestures'}
        <p class="status">Touch and pen only; the mouse keeps its clicks and drags.</p>
        {#each gestureRows as { surface, gesture, options } (`${surface}:${gesture}`)}
          {@const label = `${GESTURE_NAMES[gesture]} ${SURFACES[surface]}`}
          {@const current = mapped(settings.current.gestures, surface)[gesture] ?? 'none'}
          <div class="row">
            <span class="label">{label}</span>
            <div class="segmented" role="radiogroup" aria-label={label}>
              {#each options as action (action)}
                <button
                  role="radio"
                  aria-checked={current === action}
                  class:active={current === action}
                  onclick={() => settings.remap(surface, gesture, action)}
                >
                  {ACTIONS[action]}
                </button>
              {/each}
            </div>
          </div>
        {/each}
        <div class="actions">
          <button class="button" onclick={() => settings.set('gestures', {})}>Reset to defaults</button>
        </div>
      {:else if group === 'integrations'}
        {#if engine.integrations}
          {@const integrations = engine.integrations}
          <div class="row">
            <span class="label">Lyrics from LRCLIB</span>
            <div class="segmented" role="radiogroup" aria-label="Lyrics from LRCLIB">
              {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                <button role="radio" aria-checked={integrations.lrclib === value} class:active={integrations.lrclib === value} onclick={() => engine.setIntegrations({ lrclib: value })}>{text}</button>
              {/each}
            </div>
            <p class="status">Looks up lyrics for what plays on lrclib.net. Applies to every window and device using this engine.</p>
          </div>
          {@const lb = integrations.listenbrainz}
          <div class="row">
            <span class="label">ListenBrainz</span>
            {#if lb.user}
              <div class="segmented" role="radiogroup" aria-label="Scrobble to ListenBrainz">
                {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                  <button role="radio" aria-checked={lb.enabled === value} class:active={lb.enabled === value} onclick={() => engine.setIntegrations({ listenbrainz: { ...lb, enabled: value } })}>{text}</button>
                {/each}
              </div>
              <div class="actions">
                <p class="status">Signed in as {lb.user}.</p>
                <button class="button" onclick={() => signIn('listenbrainz', '')}>Sign out</button>
              </div>
            {:else}
              <form
                class="actions token"
                onsubmit={(e) => {
                  e.preventDefault();
                  signIn('listenbrainz', listenBrainzToken.trim());
                  listenBrainzToken = '';
                }}
              >
                <input class="select" type="password" autocomplete="off" placeholder="User token" aria-label="ListenBrainz user token" bind:value={listenBrainzToken} />
                <button class="button" disabled={!listenBrainzToken.trim()}>Sign in</button>
              </form>
              <p class="status">Paste the user token from your settings page on listenbrainz.org.</p>
            {/if}
            {#if lb.error}<p class="status error selectable">{lb.error}</p>{/if}
            <p class="status">Scrobbles what this engine plays, from any window or none.</p>
          </div>
          {#each [['lastfm', 'Last.fm'], ['librefm', 'Libre.fm']] as const as [service, name] (service)}
          {@const fm = integrations[service]}
          <div class="row">
            <span class="label">{name}</span>
            {#if fm.user}
              <div class="segmented" role="radiogroup" aria-label="Scrobble to {name}">
                {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                  <button role="radio" aria-checked={fm.enabled === value} class:active={fm.enabled === value} onclick={() => engine.setIntegrations({ [service]: { ...fm, enabled: value } })}>{text}</button>
                {/each}
              </div>
              <div class="actions">
                <p class="status">Signed in as {fm.user}.</p>
                <button class="button" onclick={() => signIn(service, '')}>Sign out</button>
              </div>
            {:else if fm.auth_url}
              {@const url = fm.auth_url}
              <div class="actions">
                <p class="status">Allow Motif on the {name} page in your browser.</p>
                <button class="button" onclick={() => openUrl(url)}>Open it again</button>
                <button class="button" onclick={() => signIn(service, '')}>Cancel</button>
              </div>
            {:else}
              <div class="actions">
                <button
                  class="button"
                  onclick={() => {
                    browserSignIn = service;
                    signIn(service, 'browser');
                  }}>Sign in with {name}</button
                >
              </div>
            {/if}
            {#if fm.error}<p class="status error selectable">{fm.error}</p>{/if}
            <p class="status">Scrobbles what this engine plays, from any window or none.</p>
          </div>
          {/each}
          <!-- The phone's own engine can't reach Discord; a computer's engine still can, from the phone. -->
          {#if !storage}
          <div class="row">
            <span class="label">Discord status</span>
            <div class="segmented" role="radiogroup" aria-label="Discord status">
              {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                <button role="radio" aria-checked={integrations.discord === value} class:active={integrations.discord === value} onclick={() => engine.setIntegrations({ discord: value })}>{text}</button>
              {/each}
            </div>
            <p class="status">Shows what plays on your Discord profile, as "Listening to" the artist, while Discord runs on the computer playing. Cover art comes from the Cover Art Archive for tracks tagged with a MusicBrainz release.</p>
          </div>
          {/if}
          {#if integrations.secrets_unencrypted}
            <p class="status">No system keyring found: sign-ins are stored unencrypted, readable only by your user.</p>
          {/if}
        {:else}
          <p class="status">Integration settings aren't available yet.</p>
        {/if}
      {:else if group === 'devices'}
        {#if engine.pairing}
          {@const p = engine.pairing}
          <div class="row">
            <span class="label">Other devices</span>
            <div class="segmented" role="radiogroup" aria-label="Other devices">
              {#each [[true, 'Allowed'], [false, 'Off']] as const as [value, text] (text)}
                <button role="radio" aria-checked={p.lan.allowed === value} class:active={p.lan.allowed === value} onclick={() => engine.pair({ type: 'set_lan', allowed: value })}>{text}</button>
              {/each}
            </div>
            <p class="status">
              {#if p.lan.open}Reachable on the network at <span class="selectable">{p.lan.addresses.join(', ')}</span>.
              {:else if p.lan.allowed}Opens to the network while a device is paired or pairing.
              {:else}Only this computer can use the engine; paired devices stay paired.{/if}
            </p>
            <div class="actions">
              <button class="button primary" onclick={() => (settingsView.pairing = true)}>Pair a device…</button>
            </div>
          </div>
          <div class="row">
            <span class="label">Paired devices</span>
            {#each p.devices as device (device.id)}
              {@const connections = p.connected.filter((c) => c.device === device.id)}
              {@const preset = presetOf(device.scopes)}
              <div class="device">
                <input
                  class="select name"
                  aria-label="Name of {device.name}"
                  value={device.name}
                  maxlength="64"
                  onchange={(e) => rename(device.id, device.name, e.currentTarget)}
                  onkeydown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                />
                <select
                  class="select preset"
                  aria-label="Access for {device.name}"
                  value={preset ?? ''}
                  onchange={(e) => engine.pair({ type: 'set_preset', device: device.id, preset: e.currentTarget.value as Preset })}
                >
                  {#if !preset}<option value="" disabled>Custom</option>{/if}
                  {#each PRESETS as [value, label] (value)}<option {value}>{label}</option>{/each}
                </select>
                <button class="button" onclick={() => (removing = device.id)}>Remove</button>
                {#if removing === device.id}
                  <p class="status confirm">
                    Remove {device.name}? It will need a new code to connect again.
                    <button class="button" onclick={() => engine.pair({ type: 'revoke', device: device.id })}>Remove</button>
                    <button class="button" onclick={() => (removing = null)}>Cancel</button>
                  </p>
                {/if}
                <p class="status seen">
                  {#if connections.length}
                    <span class="online">● Connected now</span>
                    {#each connections as c (c.id)}
                      <button class="link" title="Close this connection; the device stays paired" onclick={() => engine.pair({ type: 'disconnect', connection: c.id })}>Disconnect {c.address.replace(/:\d+$/, '')}</button>
                    {/each}
                  {:else if device.last_seen_ms}Last seen {engineTime(device.last_seen_ms)}
                  {:else}Never connected{/if}
                </p>
              </div>
            {:else}
              <p class="status">No devices yet.</p>
            {/each}
          </div>
          <div class="row">
            <span class="label">Approve new devices automatically</span>
            <select
              class="select narrow"
              aria-label="Approve new devices automatically"
              value={p.auto_approve ?? ''}
              onchange={(e) => engine.pair({ type: 'set_auto_approve', preset: (e.currentTarget.value || null) as Preset | null })}
            >
              <option value="">Off: ask every time</option>
              {#each PRESETS as [value, label] (value)}<option {value}>On, with {label}</option>{/each}
            </select>
            <p class="status">Anyone with a code gets in without asking you.</p>
          </div>
          <div class="row">
            <span class="label">MPD clients</span>
            <div class="segmented" role="radiogroup" aria-label="MPD clients">
              {#each [[true, 'On'], [false, 'Off']] as const as [value, text] (text)}
                <button role="radio" aria-checked={p.mpd.enabled === value} class:active={p.mpd.enabled === value} onclick={() => engine.pair({ type: 'set_mpd', enabled: value })}>{text}</button>
              {/each}
            </div>
            <p class="status">
              {#if !p.mpd.enabled}Lets MPD apps such as ncmpcpp or M.A.L.P. control this engine.
              {:else if !p.mpd.listening}Can't listen on port {p.mpd.port}; is another MPD server running?
              {:else if p.lan.allowed}Listening on port {p.mpd.port}, for this computer and the network.
              {:else}Listening on port {p.mpd.port}, for this computer only.{/if}
            </p>
            <form
              class="actions token"
              onsubmit={(e) => {
                e.preventDefault();
                engine.pair({ type: 'set_mpd_password', password: mpdPassword });
                mpdPassword = '';
              }}
            >
              <input class="select" type="password" autocomplete="off" placeholder={p.mpd.password_set ? 'New password' : 'Password'} aria-label="MPD password" bind:value={mpdPassword} />
              <button class="button" disabled={!mpdPassword}>Set</button>
              {#if p.mpd.password_set}
                <button class="button" type="button" onclick={() => engine.pair({ type: 'set_mpd_password', password: '' })}>Remove</button>
              {/if}
            </form>
            <p class="status">
              {#if p.mpd.password_set}Apps that send the password can control playback; others can only look.
              {:else}Without a password, MPD apps can only look, not control playback.{/if}
            </p>
          </div>
          <div class="row">
            <span class="label">Pairing log</span>
            {#each [...p.log].reverse() as entry, i (i)}
              <p class="status">{engineTime(entry.at_ms)} · {entry.device} {LOG_ACTIONS[entry.action]}</p>
            {:else}
              <p class="status">Nothing yet.</p>
            {/each}
          </div>
        {:else}
          <p class="status">Pairing isn't available yet.</p>
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

{#if settingsView.addingEngine}<AddEngineDialog onClose={() => (settingsView.addingEngine = false)} />{/if}
{#if settingsView.pairing}<PairDialog onClose={() => (settingsView.pairing = false)} />{/if}

{#snippet gainSlider(label: string, key: 'preamp_db' | 'untagged_db', db: number)}
  <div class="row">
    <span class="label">{label}</span>
    <div class="scale">
      <Slider
        {label}
        value={db + GAIN_RANGE}
        max={GAIN_RANGE * 2}
        valueText={formatDb(db)}
        onchange={(v) => setReplayGain({ [key]: Math.round(v - GAIN_RANGE) })}
      />
      <span class="status">{formatDb(db)}</span>
    </div>
  </div>
{/snippet}

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

  .device {
    display: grid;
    grid-template-columns: 1fr auto auto;
    gap: var(--space-1) var(--space-2);
    align-items: center;
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }

  .device .name {
    min-width: 0;
  }

  .device .preset {
    width: auto;
  }

  .seen {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .confirm {
    grid-column: 1 / -1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    color: var(--danger);
  }

  .online {
    color: var(--ok);
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }

  .select.narrow {
    width: auto;
    align-self: flex-start;
  }

  .token .button {
    flex: none;
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
      padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left);
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
