<script lang="ts">
  import { getApp } from '../../context';

  // Asks for a playlist's name, or to confirm deleting one (`Playlists.asking`).
  const { playlists } = getApp();
  const asking = $derived(playlists.asking);
  let value = $state('');

  function open(dialog: HTMLDialogElement) {
    value = asking?.value ?? '';
    dialog.showModal();
  }
</script>

{#if asking}
  <dialog use:open aria-label={asking.title} onclose={() => playlists.asking === asking && playlists.answer(null)}>
    <form method="dialog" onsubmit={() => playlists.answer(asking.value === null ? '' : value)}>
      <h2>{asking.title}</h2>
      <!-- First in the dialog, so showModal() focuses it. -->
      {#if asking.value !== null}<input type="text" bind:value aria-label="Name" />{/if}
      <div class="actions">
        <button type="button" class="button" onclick={() => playlists.answer(null)}>Cancel</button>
        <button class="button primary" disabled={asking.value !== null && !value.trim()}>{asking.action}</button>
      </div>
    </form>
  </dialog>
{/if}

<style>
  dialog {
    width: min(360px, calc(100vw - 32px));
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 0.5);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  h2 {
    margin: 0;
    font-size: 1em;
  }

  input {
    padding: var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
</style>
