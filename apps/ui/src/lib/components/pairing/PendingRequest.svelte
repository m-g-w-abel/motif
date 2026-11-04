<script lang="ts">
  import { PRESETS } from '../../access';
  import { getApp } from '../../context';
  import type { PendingDevice } from '../../protocol/PendingDevice';
  import type { Preset } from '../../protocol/Preset';

  /** A device waiting for approval, with Allow (as a preset) and Deny. */
  let { request }: { request: PendingDevice } = $props();

  const { engine } = getApp();
  let preset = $state<Preset>('control_playback');
</script>

<div class="request">
  <p><strong>{request.name}</strong> wants to connect <span class="muted selectable">from {request.address.replace(/:\d+$/, '')}</span></p>
  <div class="actions">
    <select class="select" aria-label="Access for {request.name}" bind:value={preset}>
      {#each PRESETS as [value, label] (value)}<option {value}>{label}</option>{/each}
    </select>
    <button class="button primary" onclick={() => engine.pair({ type: 'approve', request: request.id, preset })}>Allow</button>
    <button class="button" onclick={() => engine.pair({ type: 'deny', request: request.id })}>Deny</button>
  </div>
</div>

<style>
  .request {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  p {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .muted {
    color: var(--text-muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
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
</style>
