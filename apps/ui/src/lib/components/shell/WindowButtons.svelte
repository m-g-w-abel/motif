<script lang="ts">
  import { getApp } from '../../context';
  import { drawsButtons, windowAction } from '../../windowChrome';
  import Icon from '../Icon.svelte';

  const { settings } = getApp();
</script>

{#if settings.current.ownDecorations && drawsButtons}
  <div class="window-buttons">
    <button class="icon-button" aria-label="Minimise" title="Minimise" onclick={() => windowAction('minimize')}>
      <Icon name="minimize" size={14} />
    </button>
    <button class="icon-button" aria-label="Maximise" title="Maximise" onclick={() => windowAction('toggleMaximize')}>
      <Icon name="maximize" size={13} />
    </button>
    <button class="icon-button close" aria-label="Close" title="Close" onclick={() => windowAction('close')}>
      <Icon name="close" size={14} />
    </button>
  </div>
{/if}

<style>
  .window-buttons {
    display: flex;
    gap: var(--space-1);
    margin-left: auto;
  }

  .window-buttons .close:hover:not(:disabled) {
    background: var(--danger);
    color: #fff;
  }
</style>
