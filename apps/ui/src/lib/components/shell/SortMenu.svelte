<script lang="ts">
  import type { SortKey } from '../../protocol/SortKey';
  import type { SortRule } from '../../protocol/SortRule';
  import { onDestroy } from 'svelte';
  import { getApp } from '../../context';
  import { moveItem } from '../../reorder';
  import { SORT_KEYS } from '../../sort.svelte';
  import Icon from '../Icon.svelte';
  import ReorderList from '../ReorderList.svelte';

  // A view's sort button and its tiered rules, edited tier by tier: a
  // popover below the button on desktop, a sheet along the bottom on phones
  // (as the ⋮ sheet).
  let {
    rules,
    defaults,
    onchange,
    oneOff = null,
    onclearOneOff,
    tip,
  }: {
    rules: readonly SortRule[];
    /** What "Reset to default" goes back to. */
    defaults: readonly SortRule[];
    onchange: (rules: SortRule[]) => void;
    /** A one-off column sort showing instead of the rules. */
    oneOff?: SortRule | null;
    onclearOneOff?: () => void;
    tip?: string;
  } = $props();

  // Open state lives in `views`, so Android back closes the menu before leaving the view.
  const { views } = getApp();
  onDestroy(() => (views.sorting = false));
  let menu = $state<HTMLElement>();
  const sortKeys = Object.keys(SORT_KEYS) as SortKey[];
  const same = (a: readonly SortRule[], b: readonly SortRule[]) =>
    a.length === b.length && a.every((r, i) => r.key === b[i].key && r.descending === b[i].descending);

  const change = (i: number, rule: Partial<SortRule>) => onchange(rules.map((r, j) => (j === i ? { ...r, ...rule } : r)));
  const remove = (i: number) => onchange(rules.filter((_, j) => j !== i));
  const add = () =>
    onchange([...rules, { key: sortKeys.find((k) => !rules.some((r) => r.key === k)) ?? 'title', descending: false }]);
</script>

<svelte:window
  onpointerdown={(e) => {
    if (views.sorting && menu && !menu.contains(e.target as Node)) views.sorting = false;
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape') views.sorting = false;
  }}
/>

<div class="menu" bind:this={menu}>
  <button
    class="icon-button"
    class:active={!same(rules, defaults) || oneOff}
    aria-haspopup="dialog"
    aria-expanded={views.sorting}
    aria-label="Sort order"
    title="Sort order"
    onclick={() => (views.sorting = !views.sorting)}
  >
    <Icon name="sort" size={16} />
  </button>
  {#if views.sorting}
    <button class="scrim" tabindex="-1" aria-label="Close" onclick={() => (views.sorting = false)}></button>
    <div class="popover" role="dialog" aria-label="Sort order">
      <div class="current">Sort by</div>
      {#if oneOff}
        <p class="tip">
          Sorted by {SORT_KEYS[oneOff.key].toLowerCase()} from a column header for now.
          <button class="link" onclick={onclearOneOff}>Use these rules</button>
        </p>
      {/if}
      <div class="tiers">
        <ReorderList items={[...rules]} label="Sort tiers" onmove={(from, to) => onchange(moveItem([...rules], from, to))}>
          {#snippet row(rule, i)}
            <div class="tier">
              <select aria-label="Sort tier {i + 1}" value={rule.key} onchange={(e) => change(i, { key: e.currentTarget.value as SortKey })}>
                {#each sortKeys as key (key)}
                  <option value={key}>{SORT_KEYS[key]}</option>
                {/each}
              </select>
              <button
                class="direction"
                title={rule.descending ? 'Descending: click for ascending' : 'Ascending: click for descending'}
                onclick={() => change(i, { descending: !rule.descending })}
              >
                {rule.descending ? 'Desc' : 'Asc'}
              </button>
              <button class="icon" aria-label="Remove tier" title="Remove tier" disabled={rules.length === 1} onclick={() => remove(i)}>
                <Icon name="close" size={12} />
              </button>
            </div>
          {/snippet}
        </ReorderList>
      </div>
      {#if rules.length < sortKeys.length}<button onclick={add}>Add a tier</button>{/if}
      {#if !same(rules, defaults)}<button onclick={() => onchange([...defaults])}>Reset to default</button>{/if}
      {#if tip}<p class="tip">{tip}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .menu {
    position: relative;
    margin-left: var(--space-1);
  }

  .popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 40;
    width: 300px;
    padding: var(--space-1) 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .current {
    padding: var(--space-2) var(--space-3) var(--space-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  .popover button {
    display: block;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .popover button:hover {
    background: var(--hover);
  }

  .tiers {
    padding: 0 var(--space-3) var(--space-1);
  }

  .tier {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: 2px 0;
  }

  select {
    flex: 1;
    min-width: 0;
    height: 24px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }

  .popover .tier button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: var(--radius-sm);
    text-align: center;
    font-size: 10px;
  }

  .popover .tier .direction {
    width: 40px;
    font-size: 11px;
  }

  .popover .tier button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .popover .link {
    display: inline;
    width: auto;
    padding: 0;
    color: var(--accent);
  }

  .tip {
    margin: var(--space-1) var(--space-3) var(--space-2);
    font-size: 11px;
    color: var(--text-muted);
  }

  .scrim {
    display: none;
  }

  /* Phones: a sheet along the bottom, over the bar, like the ⋮ sheet. */
  @media (max-width: 700px) {
    .scrim {
      position: fixed;
      inset: 0;
      z-index: 40;
      display: block;
      padding: 0;
      border: 0;
      background: rgb(0 0 0 / 0.4);
      cursor: default;
    }

    .popover {
      position: fixed;
      top: auto;
      right: 0;
      bottom: 0;
      left: 0;
      z-index: 41;
      width: auto;
      max-height: 80%;
      overflow-y: auto;
      padding-bottom: calc(var(--space-3) + env(safe-area-inset-bottom));
      border-radius: var(--radius-md) var(--radius-md) 0 0;
      box-shadow: none;
    }

    .popover > button {
      min-height: 48px;
    }

    .tier {
      min-height: 44px;
    }

    select {
      height: 36px;
    }

    .popover .tier button {
      width: 36px;
      height: 36px;
    }
  }
</style>
