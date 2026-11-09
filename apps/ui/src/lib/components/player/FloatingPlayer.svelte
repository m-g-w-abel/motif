<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp } from '../../context';
  import { BAR_ART_SIZE } from '../../library.svelte';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';
  import { isFloatPos, MARGIN, place, resolve, snap, type FloatBox, type FloatPos } from './floating';
  import SeekBar from './SeekBar.svelte';
  import TrackLabel from './TrackLabel.svelte';
  import Transport from './Transport.svelte';

  const { engine, sheet } = getApp();
  const KEY_STEP = 16;
  const POSITION_KEY = 'floating-player.position';
  const DEFAULT_POS: FloatPos = { h: 'right', v: 'bottom', x: MARGIN, y: MARGIN };

  let el = $state<HTMLElement>();
  let pos = $state<FloatPos>(loadPosition());

  // An old top-left position isn't in this format and is dropped.
  function loadPosition(): FloatPos {
    try {
      const v = JSON.parse(localStorage.getItem(POSITION_KEY) ?? 'null');
      if (isFloatPos(v)) return v;
    } catch {
      // Fall through to the default.
    }
    return DEFAULT_POS;
  }

  /** The area above the bottom inset (a px value, when something sets one). */
  function box(): FloatBox {
    const inset = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--bottom-inset')) || 0;
    return {
      w: el?.offsetWidth ?? 0,
      h: el?.offsetHeight ?? 0,
      W: window.innerWidth,
      H: window.innerHeight - inset,
    };
  }

  function save() {
    try {
      localStorage.setItem(POSITION_KEY, JSON.stringify(pos));
    } catch {
      // Position just won't persist.
    }
  }

  function startDrag(e: PointerEvent) {
    if (!el || (e.target as HTMLElement).closest('button, input')) return;
    const rect = el.getBoundingClientRect();
    const dx = e.clientX - rect.left;
    const dy = e.clientY - rect.top;
    const b = box();
    // Otherwise the drag also selects text across the page.
    e.preventDefault();
    el.setPointerCapture(e.pointerId);

    const move = (ev: PointerEvent) => (pos = place(ev.clientX - dx, ev.clientY - dy, b));
    const end = () => {
      el?.removeEventListener('pointermove', move);
      pos = snap(pos, b);
      save();
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', end, { once: true });
    el.addEventListener('pointercancel', end, { once: true });
  }

  function onKey(e: KeyboardEvent) {
    const d = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
    if (!d) return;
    e.preventDefault();
    const b = box();
    const step = e.shiftKey ? KEY_STEP * 4 : KEY_STEP;
    const { left, top } = resolve(pos, b);
    pos = place(left + d[0] * step, top + d[1] * step, b);
    save();
  }

  // Edge offsets follow the window by themselves; re-clamp on resize (and on
  // load) so a smaller window never leaves it off screen. Untracked: reading
  // `pos` here would re-run this on every move, and writing it loops.
  $effect(() => {
    const onResize = () => {
      const b = box();
      const { left, top } = resolve(pos, b);
      pos = place(left, top, b);
    };
    untrack(onResize);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  });

  // The sheet morphs out of (and back into) wherever this sits. A plain
  // field, not state, so this doesn't re-run.
  $effect(() => {
    const node = el;
    sheet.origin = node ?? null;
    return () => {
      if (sheet.origin === node) sheet.origin = null;
    };
  });

  function resetPosition() {
    pos = DEFAULT_POS;
    try {
      localStorage.removeItem(POSITION_KEY);
    } catch {
      // Nothing stored.
    }
  }
</script>

<section
  bind:this={el}
  class="floating"
  aria-label="Now playing"
  style:left={pos.h === 'left' ? 0 : undefined}
  style:right={pos.h === 'right' ? 0 : undefined}
  style:top={pos.v === 'top' ? 0 : undefined}
  style:bottom={pos.v === 'bottom' ? 'var(--bottom-inset, 0px)' : undefined}
  style:visibility={sheet.open ? 'hidden' : undefined}
  style:transform={`translate(${pos.h === 'left' ? pos.x : -pos.x}px, ${pos.v === 'top' ? pos.y : -pos.y}px)`}
  onpointerdown={startDrag}
  ondblclick={resetPosition}
>
  <div class="row">
    <button class="expand" title="Expand player" onclick={() => sheet.expand()}>
      <Artwork track={engine.current?.track} size={48} exact={BAR_ART_SIZE} />
      <TrackLabel />
    </button>
    <span
      class="grip"
      role="button"
      tabindex="0"
      aria-label="Move player (arrow keys; Shift for bigger steps)"
      title="Drag to move; double-click to snap back"
      onkeydown={onKey}><Icon name="grip" size={14} /></span
    >
  </div>
  <div class="row controls">
    <Transport size={16} />
  </div>
  <SeekBar variant="strip" />
</section>

<style>
  .floating {
    position: fixed;
    z-index: 20;
    width: 300px;
    max-width: calc((100vw - 32px) / var(--scale, 1));
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    /* Opaque and unblurred: it moves over scrolling content (see CLAUDE.md). */
    background: var(--surface);
    will-change: transform;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .floating:active {
    cursor: grabbing;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
  }

  .expand {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .controls {
    justify-content: center;
  }

  .grip {
    align-self: flex-start;
    color: var(--text-muted);
  }
</style>
