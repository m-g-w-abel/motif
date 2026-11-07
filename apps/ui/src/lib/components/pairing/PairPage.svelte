<script lang="ts">
  import { guessName } from '../../deviceName';
  import { openInAppLink } from '../../discover';
  import { saveToken } from '../../engine.svelte';
  import type { Paired } from '../../protocol/Paired';
  import type { PairRequest } from '../../protocol/PairRequest';

  /**
   * The new device's side of pairing: opened from the host's QR code
   * (`/pair?code=…&id=<engine key>`), or shown when the engine refuses this
   * device (`unpaired`: never paired, or removed by the host).
   */
  let { unpaired = false }: { unpaired?: boolean } = $props();

  const params = new URLSearchParams(location.search);
  /** The engine the code came from; the answer must come from the same one. */
  const engineKey = params.get('id');
  let code = $state(params.get('code') ?? '');
  /** Opened from the QR in a phone's browser: Motif, if installed, can pair itself instead. */
  const inApp = $derived(!unpaired && params.get('code') && engineKey && /Android/.test(navigator.userAgent) ? openInAppLink(location.href) : null);
  let name = $state(guessName());
  let waiting = $state<AbortController | null>(null);
  let error = $state<string | null>(null);

  async function pair(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    const abort = new AbortController();
    waiting = abort;
    try {
      const request: PairRequest = { code: code.trim(), name: name.trim(), public_key: null };
      // Waits (up to 5 minutes) until the host allows or denies this device.
      const response = await fetch('/pair', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(request),
        signal: abort.signal,
      });
      if (!response.ok) throw new Error((await response.text()) || `the engine answered ${response.status}`);
      const paired = (await response.json()) as Paired;
      if (engineKey && paired.engine_key !== engineKey) throw new Error("this isn't the engine the code came from");
      saveToken(paired.token);
      location.replace('/');
    } catch (e) {
      if (!abort.signal.aborted) error = (e as Error).message;
    } finally {
      waiting = null;
    }
  }
</script>

<main class="pair">
  <div class="card">
    <h1>{unpaired ? 'Not paired' : 'Pair with Motif'}</h1>
    {#if waiting}
      <p>Waiting for approval…</p>
      <p class="hint">Allow <strong>{name}</strong> on the computer running Motif.</p>
      <button class="button" onclick={() => waiting?.abort()}>Cancel</button>
    {:else}
      {#if unpaired}
        <p class="hint">This device isn't paired with this Motif, or was removed. On the computer running Motif, open Settings → Devices → Pair a device, then scan the QR code or enter the code here.</p>
      {/if}
      {#if inApp}
        <a class="button primary open" href={inApp}>Open in Motif</a>
        <p class="hint">Or pair this browser:</p>
      {/if}
      <form onsubmit={pair}>
        <label>
          <span>Code</span>
          <input class="field code" bind:value={code} autocomplete="off" autocapitalize="characters" spellcheck="false" required />
        </label>
        <label>
          <span>Name for this device</span>
          <input class="field" bind:value={name} maxlength="64" required />
        </label>
        <button class="button primary" disabled={!code.trim() || !name.trim()}>Pair</button>
      </form>
      {#if error}<p class="error selectable" role="alert">{error}</p>{/if}
      {#if unpaired}<button class="button" onclick={() => location.reload()}>Try again</button>{/if}
    {/if}
  </div>
</main>

<style>
  .pair {
    box-sizing: border-box;
    min-height: 100dvh;
    display: grid;
    place-items: center;
    padding: var(--space-4);
    background: var(--bg);
  }

  .card {
    box-sizing: border-box;
    width: min(360px, 100%);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }

  h1 {
    margin: 0;
    font-size: 1.2em;
  }

  p {
    margin: 0;
  }

  .hint {
    color: var(--text-muted);
  }

  .error {
    color: var(--danger);
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

  .button {
    height: 36px;
  }

  .open {
    display: grid;
    place-items: center;
    text-decoration: none;
  }
</style>
