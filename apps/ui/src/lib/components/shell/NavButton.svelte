<script lang="ts">
  import { getApp } from '../../context';
  import { viewTitle } from '../../views.svelte';
  import Icon from '../Icon.svelte';

  // A view's leading button: back, showing where it goes. On desktop the left
  // panel is the Library home, so a category has no back but folds the panel.
  const { settings, views } = getApp();
  const parent = $derived(views.parent);
</script>

{#if parent && (views.phone || parent.kind !== 'home')}
  <button class="back" onclick={() => views.back()}>
    <Icon name="chevron-left" size={16} /><span>{viewTitle(parent)}</span>
  </button>
{:else if !views.phone}
  <button
    class="icon-button"
    aria-label="Toggle navigation"
    title="Toggle navigation (Ctrl+B)"
    onclick={() => settings.set('navCollapsed', !settings.current.navCollapsed)}
  >
    <Icon name="list" size={18} />
  </button>
{/if}

<style>
  .back {
    display: flex;
    align-items: center;
    gap: 2px;
    max-width: 40%;
    height: 28px;
    margin-left: calc(-1 * var(--space-2));
    padding: 0 var(--space-2) 0 0;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: var(--accent);
    cursor: pointer;
    flex: none;
  }

  .back span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .back:hover {
    background: var(--hover);
  }

  .icon-button {
    margin-left: calc(-1 * var(--space-2));
  }
</style>
