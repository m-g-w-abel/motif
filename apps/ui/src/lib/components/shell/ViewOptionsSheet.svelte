<script lang="ts">
  import { getApp } from '../../context';
  import type { MenuItem } from '../../contextMenu.svelte';
  import { launcherItems } from '../../launcher';
  import { MAX_COLUMNS, viewOptions, type OptionsKey, type ViewMode } from '../../viewOptions';
  import type { View } from '../../views.svelte';
  import { dim, rise } from '../../screens';

  // The ⋮ sheet of a collection view: its display options and library
  // actions (sorting has its own button). A sheet from the bottom on phones, a panel below ⋮ on desktop.
  let {
    title,
    view,
    cover = null,
    items = [],
    key,
    shownColumns,
    artistsLabel = 'Show artists',
    onclose,
  }: {
    title: string;
    /** The view the sheet is for, which Add to launcher opens. */
    view: View;
    /** The artwork URL Add to launcher's shortcut shows. */
    cover?: string | null;
    /** The view's own actions (a playlist's Rename, Delete…). */
    items?: MenuItem[];
    key: OptionsKey;
    /** The columns the grid shows now, which "Auto" may have picked. */
    shownColumns: number;
    artistsLabel?: string;
    onclose: () => void;
  } = $props();

  const { engine, settings } = getApp();
  const o = $derived(viewOptions(settings.current.viewOptions, key));
  const set = (patch: Parameters<typeof settings.setView>[1]) => settings.setView(key, patch);

  const MODES: [ViewMode, string][] = [
    ['grid', 'Grid'],
    ['list', 'List'],
    ['covers', 'List + covers'],
  ];

  const rescan = () => engine.query({ type: 'rescan' }).catch((e: Error) => (engine.error = e.message));

  function focusFirst(node: HTMLElement) {
    node.querySelector<HTMLElement>('button')?.focus();
  }
</script>

<button class="scrim" tabindex="-1" aria-label="Close" onclick={onclose} transition:dim></button>
<div
  transition:rise
  class="sheet"
  role="dialog"
  aria-label="{title} options"
  tabindex="-1"
  use:focusFirst
  onkeydown={(e) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onclose();
    }
  }}
>
  <h2>{title}</h2>
  <div class="row">
    <span>View</span>
    <div class="segmented" role="radiogroup" aria-label="View">
      {#each MODES as [mode, label] (mode)}
        <button role="radio" aria-checked={o.mode === mode} class:active={o.mode === mode} onclick={() => set({ mode })}>
          {label}
        </button>
      {/each}
    </div>
  </div>
  {#if o.mode === 'grid'}
    <div class="row">
      <span>Columns</span>
      <div class="step">
        {#if o.columns !== null}<button class="auto" onclick={() => set({ columns: null })}>Auto</button>{/if}
        <button aria-label="Fewer columns" disabled={shownColumns <= 1} onclick={() => set({ columns: shownColumns - 1 })}>−</button>
        <b aria-live="polite">{shownColumns}</b>
        <button aria-label="More columns" disabled={shownColumns >= MAX_COLUMNS} onclick={() => set({ columns: shownColumns + 1 })}>
          +
        </button>
      </div>
    </div>
    <label class="row">
      <span>Show titles</span>
      <input type="checkbox" role="switch" checked={o.titles} onchange={(e) => set({ titles: e.currentTarget.checked })} />
    </label>
    <label class="row">
      <span>{artistsLabel}</span>
      <input type="checkbox" role="switch" checked={o.artists} onchange={(e) => set({ artists: e.currentTarget.checked })} />
    </label>
  {/if}
  <hr />
  {#each [...items, ...launcherItems(view, cover)] as item (item.label)}
    <button class="item" onclick={() => (item.run(), onclose())}>{item.label}</button>
  {/each}
  {#if engine.can('library')}
    <button class="item" onclick={() => (void rescan(), onclose())}>Rescan library</button>
  {/if}
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 40;
    padding: 0;
    border: 0;
    background: none;
    cursor: default;
  }

  .sheet {
    position: absolute;
    top: calc(100% + var(--space-1));
    right: var(--space-2);
    z-index: 41;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    width: min(340px, calc(100% - 2 * var(--space-2)));
    padding: var(--space-2) 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface);
    color: var(--text);
  }

  h2 {
    margin: 0;
    padding: var(--space-1) var(--space-3) var(--space-2);
    font-size: 1em;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 40px;
    padding: 0 var(--space-3);
  }

  label.row {
    cursor: pointer;
  }

  .segmented {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    overflow: hidden;
  }

  .segmented button {
    height: 28px;
    padding: 0 var(--space-2);
    border: 0;
    border-right: 1px solid var(--border);
    background: none;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  .segmented button:last-child {
    border-right: 0;
  }

  .segmented button.active {
    background: var(--accent);
    color: var(--accent-contrast);
    font-weight: 600;
  }

  .step {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .step button {
    min-width: 32px;
    height: 32px;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: none;
    cursor: pointer;
  }

  .step button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .step .auto {
    padding: 0 var(--space-2);
    font-size: 12px;
  }

  .step b {
    min-width: 2ch;
    text-align: center;
  }

  input[type='checkbox'] {
    width: 18px;
    height: 18px;
    accent-color: var(--accent);
  }

  hr {
    width: 100%;
    margin: var(--space-1) 0;
    border: 0;
    border-top: 1px solid var(--border);
  }

  .item {
    min-height: 40px;
    padding: 0 var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .item:hover {
    background: var(--hover);
  }

  /* Phones: a sheet along the bottom, over the bar. */
  @media (max-width: 700px) {
    .scrim {
      background: rgb(0 0 0 / 0.4);
    }

    .sheet {
      position: fixed;
      top: auto;
      right: 0;
      bottom: 0;
      left: 0;
      width: auto;
      padding-bottom: calc(var(--space-3) + env(safe-area-inset-bottom));
      border-radius: var(--radius-md) var(--radius-md) 0 0;
    }

    .row,
    .item {
      min-height: 48px;
    }
  }
</style>
