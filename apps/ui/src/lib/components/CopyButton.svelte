<script lang="ts">
  import Icon from './Icon.svelte';

  let { text }: { text: string } = $props();
  let copied = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy(e: MouseEvent) {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      return;
    }
    copied = true;
    clearTimeout(timer);
    timer = setTimeout(() => (copied = false), 1500);
  }
</script>

<button class="icon-button" aria-label="Copy error" title={copied ? 'Copied' : 'Copy error'} onclick={copy}>
  {#if copied}<span class="copied">Copied</span>{:else}<Icon name="copy" size={16} />{/if}
</button>

<style>
  .icon-button {
    flex: none;
    width: auto;
    min-width: 32px;
    padding: 0 var(--space-1);
  }

  .copied {
    font-size: 11px;
  }
</style>
