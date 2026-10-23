<script lang="ts">
  import { getApp } from '../../context';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';
  import SeekBar from './SeekBar.svelte';
  import TrackLabel from './TrackLabel.svelte';
  import Transport from './Transport.svelte';

  const { engine, sheet } = getApp();
  const MARGIN = 16;
  const POSITION_KEY = 'floating-player.position';

  let el = $state<HTMLElement>();
  /** Top-left in px once dragged; until then it sits bottom-right. */
  let dragged = $state<{ x: number; y: number } | null>(loadPosition());

  function loadPosition(): { x: number; y: number } | null {
    try {
      return JSON.parse(localStorage.getItem(POSITION_KEY) ?? 'null');
    } catch {
      return null;
    }
  }

  function clamp(p: { x: number; y: number }) {
    const w = el?.offsetWidth ?? 0;
    const h = el?.offsetHeight ?? 0;
    return {
      x: Math.min(Math.max(MARGIN, p.x), window.innerWidth - w - MARGIN),
      y: Math.min(Math.max(MARGIN, p.y), window.innerHeight - h - MARGIN),
    };
  }

  function startDrag(e: PointerEvent) {
    if (!el || (e.target as HTMLElement).closest('button, input')) return;
    const rect = el.getBoundingClientRect();
    const dx = e.clientX - rect.left;
    const dy = e.clientY - rect.top;
    el.setPointerCapture(e.pointerId);

    const move = (ev: PointerEvent) => (dragged = clamp({ x: ev.clientX - dx, y: ev.clientY - dy }));
    const end = () => {
      el?.removeEventListener('pointermove', move);
      try {
        localStorage.setItem(POSITION_KEY, JSON.stringify(dragged));
      } catch {
        // Position just won't persist.
      }
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', end, { once: true });
    el.addEventListener('pointercancel', end, { once: true });
  }

  // Keep a dragged widget on screen when the window shrinks, or when it
  // opens smaller than when the position was saved.
  $effect(() => {
    const onResize = () => dragged && (dragged = clamp(dragged));
    onResize();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  });

  function resetPosition() {
    dragged = null;
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
  style:left={dragged ? `${dragged.x}px` : undefined}
  style:right={dragged ? undefined : `${MARGIN}px`}
  style:top={dragged ? `${dragged.y}px` : undefined}
  style:bottom={dragged ? undefined : `calc(${MARGIN}px + var(--bottom-inset, 0px))`}
  onpointerdown={startDrag}
  ondblclick={resetPosition}
>
  <div class="row">
    <button class="expand" title="Expand player" onclick={() => sheet.expand()}>
      <Artwork track={engine.current?.track} size={48} />
      <TrackLabel />
    </button>
    <span class="grip" title="Drag to move; double-click to snap back"><Icon name="grip" size={14} /></span>
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
    max-width: calc(100vw - 32px);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: color-mix(in srgb, var(--surface) 92%, transparent);
    backdrop-filter: blur(12px);
    box-shadow: var(--shadow);
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
