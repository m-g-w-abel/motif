<script lang="ts">
  import changelog from '../../../../../../CHANGELOG.md?raw';
  import { renderMarkdown } from '../../markdown';

  let { onClose }: { onClose: () => void } = $props();
  // The repo's own file, escaped by the renderer.
  const html = renderMarkdown(changelog);

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
  <div class="sheet selectable" role="dialog" aria-label="Changelog" tabindex="-1" use:focus>
    <button class="icon-button close" aria-label="Close" onclick={onClose}>×</button>
    {@html html}
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
    position: relative;
    box-sizing: border-box;
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100dvh - 32px - env(safe-area-inset-top) - env(safe-area-inset-bottom));
    overflow-y: auto;
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
    outline: none;
    line-height: 1.5;
  }

  .close {
    position: absolute;
    top: var(--space-3);
    right: var(--space-3);
    font-size: 18px;
  }

  .sheet :global(h1) {
    margin: 0 0 var(--space-2);
    font-size: 1.3em;
  }

  .sheet :global(h2) {
    margin: var(--space-4) 0 var(--space-2);
    font-size: 1.05em;
  }

  .sheet :global(p) {
    margin: 0 0 var(--space-2);
    color: var(--text-muted);
  }

  .sheet :global(ul) {
    margin: 0;
    padding-left: 1.2em;
  }

  .sheet :global(li) {
    margin-bottom: var(--space-1);
  }
</style>
