<script lang="ts">
  import { dim, pop } from '../../screens';
  import { untrack } from 'svelte';
  import { renderSVG } from 'uqr';
  import { getApp } from '../../context';
  import { formatTime } from '../../format';
  import CopyButton from '../CopyButton.svelte';
  import PendingRequest from './PendingRequest.svelte';

  /** Pairing a new device: a QR code of the pairing URL, the code, and requests waiting for approval. */
  let { onClose }: { onClose: () => void } = $props();

  const { engine } = getApp();
  const pairing = $derived(engine.pairing);
  const code = $derived(pairing?.code ?? null);

  // The countdown; ticks once a second while the dialog is open.
  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });
  const left = $derived(code ? code.expires_at_ms - (now + engine.clockOffset) : 0);
  const expired = $derived(!code || left <= 0);

  // Opening the dialog makes a code; closing it withdraws the code (a device that already used it still waits).
  let started = false;
  $effect(() => {
    if (started || engine.status !== 'open') return;
    started = true;
    const live = untrack(() => engine.pairing?.code);
    if (!live || live.expires_at_ms <= Date.now() + engine.clockOffset) engine.pair({ type: 'start' });
  });
  function close() {
    if (!expired) engine.pair({ type: 'cancel' });
    onClose();
  }

  // Other machines see a choice of addresses only when this one has several (VPN, containers).
  let chosen = $state(0);
  const urls = $derived(pairing?.lan.pair_urls ?? []);
  const url = $derived(urls[Math.min(chosen, urls.length - 1)] ?? null);
  const qr = $derived(url ? renderSVG(url, { border: 2 }) : '');

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    close();
  }

  function focus(node: HTMLElement) {
    node.focus();
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" transition:dim onclick={(e) => e.target === e.currentTarget && close()}>
  <div in:pop out:pop class="sheet" role="dialog" aria-label="Pair a device" tabindex="-1" use:focus>
    <h2>Pair a device</h2>
    {#each pairing?.pending ?? [] as request (request.id)}
      <PendingRequest {request} />
    {/each}
    {#if code && !expired}
      <p class="hint">Scan this with the new device's camera, or open the address on it.</p>
      {#if url}
        <!-- Dark on light whatever the theme: scanners expect it. -->
        <div class="qr">{@html qr}</div>
        {#if urls.length > 1}
          <select class="select" aria-label="Address" bind:value={chosen}>
            {#each pairing?.lan.addresses ?? [] as address, i (address)}<option value={i}>{address}</option>{/each}
          </select>
        {/if}
        <div class="url"><span class="selectable">{url}</span><CopyButton text={url} /></div>
      {:else}
        <p class="hint">Opening this computer to the network…</p>
      {/if}
      <p class="code">Code <strong class="selectable">{code.code.slice(0, 4)} {code.code.slice(4)}</strong></p>
      <p class="hint">Expires in {formatTime(left)}</p>
    {:else if !pairing?.pending.length}
      <p class="hint">{code ? 'The code expired.' : 'The code was used or withdrawn.'}</p>
    {/if}
    <div class="actions">
      <button class="button" onclick={() => engine.pair({ type: 'start' })}>New code</button>
      <button class="button primary" onclick={close}>Done</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    padding: env(safe-area-inset-top) env(safe-area-inset-right) env(safe-area-inset-bottom) env(safe-area-inset-left);
    display: grid;
    place-items: center;
    background: color-mix(in srgb, var(--bg) 70%, transparent);
  }

  .sheet {
    box-sizing: border-box;
    width: min(400px, calc(100vw - 32px));
    max-height: calc(100dvh - 32px - env(safe-area-inset-top) - env(safe-area-inset-bottom));
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: stretch;
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

  .qr {
    align-self: center;
    width: 200px;
    height: 200px;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .url {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .code strong {
    font-family: var(--font-mono);
    font-size: 1.3em;
    letter-spacing: 0.1em;
  }

  .select {
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
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
