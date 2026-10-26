<script lang="ts">
  import { getApp } from '../../context';
  import type { LayoutError } from '../../layout/parse';
  import Icon from '../Icon.svelte';
  import Scrollbar from '../Scrollbar.svelte';

  let { onClose }: { onClose: () => void } = $props();

  const { layout } = getApp();

  let dialog = $state<HTMLDialogElement>();
  let textarea = $state<HTMLTextAreaElement>();
  let gutter = $state<HTMLDivElement>();
  let errorList = $state<HTMLElement>();
  let draft = $state(layout.active.text);
  let errors = $state<LayoutError[]>([]);
  let applied = $state(false);

  const lineCount = $derived(draft.split('\n').length);
  const errorLines = $derived(new Set(errors.map((e) => e.line)));
  const dirty = $derived(draft !== layout.active.text);

  $effect(() => {
    dialog?.showModal();
  });

  function apply() {
    const result = layout.saveEdit(draft);
    errors = result.ok ? [] : result.errors;
    applied = result.ok;
    if (result.ok) {
      // Saving a built-in creates a copy; keep editing that copy.
      draft = layout.active.text;
      setTimeout(() => (applied = false), 1500);
    }
  }

  function revert() {
    draft = layout.active.text;
    errors = [];
  }

  /** Moves the cursor to an error's line. */
  function jumpTo(error: LayoutError) {
    if (!textarea || error.line === null) return;
    const lines = draft.split('\n');
    const offset = lines.slice(0, error.line - 1).reduce((n, l) => n + l.length + 1, 0) + (error.column ?? 1) - 1;
    textarea.focus();
    textarea.setSelectionRange(offset, offset);
    const lineHeight = parseFloat(getComputedStyle(textarea).lineHeight);
    textarea.scrollTop = Math.max(0, (error.line - 4) * lineHeight);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      apply();
    }
    // Tab inserts indentation instead of leaving the editor.
    if (e.key === 'Tab' && textarea && !e.shiftKey) {
      e.preventDefault();
      const { selectionStart: start, selectionEnd: end } = textarea;
      draft = draft.slice(0, start) + '    ' + draft.slice(end);
      requestAnimationFrame(() => textarea?.setSelectionRange(start + 4, start + 4));
    }
  }
</script>

<dialog bind:this={dialog} class="editor" aria-label="Edit layout" onclose={onClose}>
  <header>
    <h2>{layout.active.fileName}</h2>
    <span class="hint">
      {layout.active.builtin ? 'Built-in: applying saves a custom copy' : 'Your layout'} · see docs/LAYOUT.md · Ctrl+Enter to apply
    </span>
    <button class="icon-button" aria-label="Close" onclick={() => dialog?.close()}>
      <Icon name="close" size={16} />
    </button>
  </header>

  <div class="code">
    <div class="gutter" bind:this={gutter} aria-hidden="true">
      {#each { length: lineCount } as _, i (i)}
        <span class:error={errorLines.has(i + 1)}>{i + 1}</span>
      {/each}
    </div>
    <textarea
      bind:this={textarea}
      bind:value={draft}
      spellcheck="false"
      autocomplete="off"
      autocapitalize="off"
      aria-label="Layout file"
      onkeydown={onKeydown}
      onscroll={() => gutter && textarea && (gutter.scrollTop = textarea.scrollTop)}
    ></textarea>
  </div>

  {#if errors.length}
    <ul class="errors" role="alert" bind:this={errorList}>
      {#each errors as error, i (i)}
        <li>
          <button onclick={() => jumpTo(error)}>
            {#if error.line !== null}<span class="where">Line {error.line}</span>{/if}
            {error.message}
          </button>
        </li>
      {/each}
    </ul>
    <Scrollbar target={errorList} />
  {/if}

  <footer>
    <button class="button" disabled={!dirty} onclick={revert}>Revert</button>
    <span class="status">{applied ? 'Applied' : dirty ? 'Unsaved changes' : ''}</span>
    <button class="button" onclick={() => dialog?.close()}>Close</button>
    <button class="button primary" disabled={!dirty} onclick={apply}>Apply</button>
  </footer>
</dialog>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    width: min(760px, calc(100vw - 32px));
    height: min(680px, calc(100vh - 32px));
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  .editor::backdrop {
    background: rgb(0 0 0 / 0.5);
  }

  header,
  footer {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    flex: none;
  }

  header {
    border-bottom: 1px solid var(--border);
  }

  footer {
    border-top: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 1em;
  }

  .hint {
    flex: 1;
    font-size: 11px;
    color: var(--text-muted);
  }

  .code {
    flex: 1;
    min-height: 0;
    display: flex;
    background: var(--bg);
    font-family: var(--font-mono);
    font-size: 12.5px;
    line-height: 20px;
  }

  .gutter {
    flex: none;
    display: flex;
    flex-direction: column;
    padding: var(--space-2) 0;
    overflow: hidden;
    border-right: 1px solid var(--border);
    color: var(--text-muted);
    text-align: right;
    user-select: none;
  }

  .gutter span {
    min-width: 3.5em;
    padding: 0 var(--space-2);
  }

  .gutter span.error {
    background: color-mix(in srgb, var(--danger) 25%, transparent);
    color: var(--text);
  }

  textarea {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: var(--space-2) var(--space-3);
    border: 0;
    outline: none;
    resize: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    line-height: inherit;
    white-space: pre;
    tab-size: 4;
  }

  .errors {
    max-height: 30%;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
    border-top: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 8%, var(--surface));
  }

  .errors button {
    display: block;
    width: 100%;
    padding: var(--space-1) var(--space-3);
    border: 0;
    background: none;
    text-align: left;
    font-family: var(--font-mono);
    font-size: 12px;
    cursor: pointer;
  }

  .errors button:hover {
    background: var(--hover);
  }

  .where {
    margin-right: var(--space-2);
    color: var(--danger);
    font-weight: 600;
  }

  .status {
    flex: 1;
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
