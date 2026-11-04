<script lang="ts">
  import { contextMenu } from '../contextMenu.svelte';

  // Clamps the menu into the window, opening upwards from `y` when it doesn't
  // fit below, and puts the focus on its first item.
  function place(node: HTMLElement) {
    const height = node.offsetHeight;
    const top = contextMenu.y + height > innerHeight ? contextMenu.y - height : contextMenu.y;
    node.style.left = `${Math.max(0, Math.min(contextMenu.x, innerWidth - node.offsetWidth))}px`;
    node.style.top = `${Math.max(0, Math.min(top, innerHeight - height))}px`;
    node.querySelector('button')?.focus();
  }

  function onKey(e: KeyboardEvent) {
    if (!contextMenu.items.length) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation(); // Settings and the sheet also close on Esc
      contextMenu.close(true);
    }
  }

  function onMenuKey(e: KeyboardEvent) {
    const buttons = [...(e.currentTarget as HTMLElement).querySelectorAll('button')];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    const step = { ArrowDown: 1, ArrowUp: -1 }[e.key as 'ArrowDown'];
    if (step) buttons[(at + step + buttons.length) % buttons.length]?.focus();
    else if (e.key === 'Home') buttons[0]?.focus();
    else if (e.key === 'End') buttons.at(-1)?.focus();
    else if (e.key === 'Tab') contextMenu.close();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window
  onkeydowncapture={onKey}
  onpointerdowncapture={(e) => !(e.target as Element).closest?.('.context-menu') && contextMenu.close()}
  onblur={() => contextMenu.close()}
/>

{#if contextMenu.items.length}
  <!-- Keyed by position so a second right-click re-places the menu. -->
  {#key `${contextMenu.x},${contextMenu.y}`}
    <div class="context-menu" role="menu" tabindex="-1" use:place onkeydown={onMenuKey}>
      {#each contextMenu.items as item (item.label)}
        <button
          role="menuitem"
          onclick={() => {
            contextMenu.close(true);
            item.run();
          }}
        >
          {item.label}
        </button>
      {/each}
    </div>
  {/key}
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 47; /* Over the phone drawer (46), which has menus of its own. */
    min-width: 180px;
    padding: var(--space-1) 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  button {
    display: block;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  button:hover,
  button:focus-visible {
    background: var(--hover);
  }
</style>
