<script lang="ts">
  import ShortcutList from './ShortcutList.svelte';

  let { onClose }: { onClose: () => void } = $props();

  // Closes before Settings or the player sheet see the Esc.
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
<div class="backdrop" onclick={(e) => e.target === e.currentTarget && onClose()}>
  <div class="sheet" role="dialog" aria-label="Keyboard shortcuts" tabindex="-1" use:focus>
    <h2>Keyboard shortcuts</h2>
    <ShortcutList />
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
    width: min(480px, calc(100vw - 32px));
    max-height: calc(100dvh - 32px);
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
    outline: none;
  }

  h2 {
    margin: 0 0 var(--space-3);
    font-size: 1.1em;
  }
</style>
