<script lang="ts">
  import { getApp } from '../../context';
  import { SHORTCUT_GROUPS, SHORTCUTS } from '../../shortcuts';

  const { settings } = getApp();
</script>

{#each SHORTCUT_GROUPS as group (group.id)}
  <section>
    <h3>{group.label}</h3>
    <dl>
      {#each SHORTCUTS.filter((s) => s.group === group.id && (!s.vim || settings.current.vimKeys)) as shortcut (shortcut.id)}
        <dt>{shortcut.label}</dt>
        <dd>
          {#each shortcut.keys as key, i (key)}
            {#if i > 0}<span class="or">or</span>{/if}<kbd>{key}</kbd>
          {/each}
        </dd>
      {/each}
    </dl>
  </section>
{/each}

<style>
  section {
    margin-bottom: var(--space-3);
  }

  h3 {
    margin: 0 0 var(--space-1);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-muted);
  }

  dl {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: var(--space-1) var(--space-3);
    margin: 0;
  }

  dd {
    margin: 0;
    text-align: right;
  }

  kbd {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm, 4px);
    background: var(--surface);
    font: inherit;
    font-size: 0.9em;
  }

  .or {
    margin: 0 var(--space-1);
    color: var(--text-muted);
  }
</style>
