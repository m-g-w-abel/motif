<script lang="ts">
  import { dim, pop } from '../../screens';
  import { getApp } from '../../context';
  import { guessName } from '../../deviceName';
  import { discoverEngines, holdMulticast, pairEngine, parsePairLink, type FoundEngine, type PairLink } from '../../discover';
  import { engines } from '../../engine.svelte';
  import { addEngine, switchTo } from '../../engines';
  import { isAndroid } from '../../platform';

  /** Add an engine from the LAN: pick one found over mDNS and type the code its host shows (or scan its QR), wait for the host to allow it. */
  let { onClose }: { onClose: () => void } = $props();
  const { settingsView } = getApp();

  let found = $state.raw<FoundEngine[]>([]);
  let searched = $state(false);
  let failed = $state<string | null>(null);
  let chosen = $state<FoundEngine | null>(null);
  let code = $state('');
  let name = $state(guessName());
  let waiting = $state(false);
  let cancelled = false;
  let error = $state<string | null>(null);
  let cameraDenied = $state(false);

  // Looks again as long as the dialog is open; closing it stops all multicast.
  $effect(() => {
    let open = true;
    holdMulticast(true);
    (async () => {
      while (open) {
        try {
          found = await discoverEngines();
          failed = null;
        } catch (e) {
          failed = String(e);
          await new Promise((resolve) => setTimeout(resolve, 2000));
        }
        searched = true;
      }
    })();
    return () => {
      open = false;
      holdMulticast(false);
    };
  });

  const saved = (id: string) => engines.list.engines.some((e) => e.id === id);

  async function pair(e?: SubmitEvent) {
    e?.preventDefault();
    if (!chosen) return;
    error = null;
    cancelled = false;
    waiting = true;
    // Tries the engine's addresses in turn; only "can't reach" moves on to the next.
    let reason = '';
    for (const address of chosen.addresses) {
      try {
        const paired = await pairEngine(address, code, name);
        if (paired.engine_key !== chosen.id) throw new Error("this isn't the engine that was picked");
        const engine = { id: chosen.id, name: chosen.name, addresses: chosen.addresses, token: paired.token };
        // Allowed after Cancel: keep the token, but don't take control from under the user.
        engines.update((list) => (cancelled ? addEngine(list, engine) : switchTo(addEngine(list, engine), engine.id)));
        if (!cancelled) onClose();
        return;
      } catch (err) {
        reason = String(err instanceof Error ? err.message : err);
        if (!reason.startsWith("can't reach")) break;
      }
    }
    waiting = false;
    if (!cancelled) error = reason.charAt(0).toUpperCase() + reason.slice(1);
  }

  async function scanQr() {
    error = null;
    cameraDenied = false;
    let content: string;
    try {
      const scanner = await import('@tauri-apps/plugin-barcode-scanner');
      let permission = await scanner.checkPermissions();
      if (permission !== 'granted') permission = await scanner.requestPermissions();
      if (permission !== 'granted') {
        cameraDenied = true;
        return;
      }
      settingsView.scanning = true;
      content = (await scanner.scan({ formats: [scanner.Format.QRCode] })).content;
    } catch (err) {
      // Plugins reject with `{ message }`. Backing out of the camera rejects too; that needs no message.
      const reason = (err as { message?: string } | null)?.message ?? String(err);
      if (!/cancel/i.test(reason)) error = `Can't scan: ${reason}`;
      return;
    } finally {
      settingsView.scanning = false;
    }
    const link = parsePairLink(content);
    if (!link) {
      error = "That isn't a Motif pairing QR. On the other computer: Settings → Devices → Pair a device.";
      return;
    }
    await pairLink(link);
  }

  // A pairing link the app was opened with, from the host's pairing page.
  $effect(() => {
    const link = settingsView.pairLink;
    if (!link) return;
    settingsView.pairLink = null;
    pairLink(link);
  });

  async function pairLink(link: PairLink) {
    // Seen on the LAN too: its name, and its other addresses to fall back on.
    const known = found.find((e) => e.id === link.id);
    chosen = { id: link.id, name: known?.name ?? link.address, addresses: [link.address, ...(known?.addresses.filter((a) => a !== link.address) ?? [])] };
    code = link.code;
    await pair();
  }

  /** The request can't be recalled: if the host allows it later, the engine is added to the list (not switched to). */
  function cancel() {
    cancelled = true;
    waiting = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    onClose();
  }

  function focus(node: HTMLElement) {
    node.focus();
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" transition:dim onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div in:pop out:pop class="sheet" role="dialog" aria-label="Add an engine" tabindex="-1" use:focus>
    <h2>Add an engine</h2>
    {#if waiting && chosen}
      <p>Waiting for approval…</p>
      <p class="hint">Allow <strong>{name}</strong> on {chosen.name}.</p>
      <div class="actions"><button class="button" onclick={cancel}>Cancel</button></div>
    {:else if chosen}
      <p class="hint">On {chosen.name}: Settings → Devices → Pair a device, then enter the code it shows.</p>
      <form onsubmit={pair}>
        <label>
          <span>Code</span>
          <input class="field code" bind:value={code} autocomplete="off" autocapitalize="characters" spellcheck="false" use:focus />
        </label>
        <label>
          <span>Name for this device</span>
          <input class="field" bind:value={name} maxlength="64" />
        </label>
        {#if error}<p class="error selectable" role="alert">{error}</p>{/if}
        <div class="actions">
          <button type="button" class="button" onclick={() => ((chosen = null), (error = null))}>Back</button>
          <button class="button primary" disabled={!code.trim() || !name.trim()}>Pair</button>
        </div>
      </form>
    {:else}
      <p class="hint">Engines on this network:</p>
      {#each found as engine (engine.id)}
        <button class="engine" disabled={saved(engine.id)} onclick={() => ((chosen = engine), (code = ''))}>
          <span>{engine.name}</span>
          <span class="hint">{saved(engine.id) ? 'Added' : engine.addresses[0]}</span>
        </button>
      {/each}
      {#if failed}
        <p class="error selectable" role="alert">Can't look for engines: {failed}</p>
      {:else if !found.length}
        <p class="hint">{searched ? 'None found yet. Both devices need to be on the same network, and the other computer must be allowed to open to it (Settings → Devices).' : 'Looking…'}</p>
      {/if}
      {#if cameraDenied}
        <p class="error" role="alert">Motif isn't allowed to use the camera. Allow it in Android's settings: Permissions → Camera.</p>
      {/if}
      {#if error}<p class="error selectable" role="alert">{error}</p>{/if}
      <div class="actions">
        {#if cameraDenied}
          <button class="button" onclick={() => import('@tauri-apps/plugin-barcode-scanner').then((s) => s.openAppSettings())}>Open settings</button>
        {/if}
        {#if isAndroid}
          <button class="button" onclick={scanQr}>Scan QR</button>
        {/if}
        <button class="button primary" onclick={onClose}>Done</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
  }

  .sheet {
    box-sizing: border-box;
    width: min(400px, calc(100vw - 32px));
    max-height: calc(100dvh - 32px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
    outline: none;
  }

  h2 {
    margin: 0;
    font-size: 1.1em;
  }

  p {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
    font-size: 12px;
  }

  .error {
    color: var(--danger);
  }

  .engine {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-2);
    min-height: 44px;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    text-align: left;
  }

  .engine:disabled {
    opacity: 0.6;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: 12px;
    color: var(--text-muted);
  }

  .field {
    height: 36px;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
    /* 16px keeps phones from zooming in on focus. */
    font: inherit;
    font-size: 16px;
  }

  .code {
    font-family: var(--font-mono);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
