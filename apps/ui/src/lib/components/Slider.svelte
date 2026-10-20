<script lang="ts">
  /**
   * A horizontal slider drawn with plain elements, so it looks and behaves the
   * same in every webview (native range inputs differ between WebKit and
   * Chromium). `oninput` fires while dragging, `onchange` once on release.
   */
  let {
    value,
    max,
    label,
    valueText,
    disabled = false,
    variant = 'normal',
    oninput,
    onchange,
  }: {
    value: number;
    max: number;
    label: string;
    /** Spoken value, e.g. "1:45 of 3:07". */
    valueText?: string;
    disabled?: boolean;
    /** `strip`: a thin edge-to-edge bar that thickens on hover. */
    variant?: 'normal' | 'strip' | 'muted';
    oninput?: (value: number) => void;
    onchange?: (value: number) => void;
  } = $props();

  let track = $state<HTMLElement>();
  /** Set while dragging, so outside updates don't fight the pointer. */
  let dragValue = $state<number | null>(null);
  const shown = $derived(dragValue ?? value);
  const fraction = $derived(max > 0 ? Math.min(1, Math.max(0, shown / max)) : 0);

  function valueAt(clientX: number): number {
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width)) * max;
  }

  function onPointerDown(e: PointerEvent) {
    if (disabled || e.button !== 0 || !track) return;
    e.preventDefault();
    track.setPointerCapture(e.pointerId);
    dragValue = valueAt(e.clientX);
    oninput?.(dragValue);
  }

  function onPointerMove(e: PointerEvent) {
    if (dragValue === null) return;
    dragValue = valueAt(e.clientX);
    oninput?.(dragValue);
  }

  function onPointerUp() {
    if (dragValue === null) return;
    const final = dragValue;
    dragValue = null;
    onchange?.(final);
  }

  function onKey(e: KeyboardEvent) {
    if (disabled) return;
    const step = max / 20;
    const next =
      e.key === 'ArrowRight' || e.key === 'ArrowUp'
        ? value + step
        : e.key === 'ArrowLeft' || e.key === 'ArrowDown'
          ? value - step
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? max
              : null;
    if (next === null) return;
    e.preventDefault();
    const clamped = Math.min(max, Math.max(0, next));
    oninput?.(clamped);
    onchange?.(clamped);
  }
</script>

<div
  bind:this={track}
  class="slider {variant}"
  class:disabled
  class:dragging={dragValue !== null}
  role="slider"
  tabindex={disabled ? -1 : 0}
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={max}
  aria-valuenow={Math.round(shown)}
  aria-valuetext={valueText}
  aria-disabled={disabled}
  style:--fraction={fraction}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  onkeydown={onKey}
>
  <div class="rail"><div class="fill"></div></div>
  <div class="thumb"></div>
</div>

<style>
  .slider {
    position: relative;
    display: flex;
    align-items: center;
    width: 100%;
    min-width: 0;
    height: 16px;
    cursor: pointer;
    touch-action: none;
    user-select: none;
  }

  .slider.disabled {
    cursor: default;
    opacity: 0.5;
  }

  .rail {
    position: relative;
    width: 100%;
    height: 3px;
    background: var(--border);
    overflow: hidden;
    transition: height 120ms ease;
  }

  .fill {
    height: 100%;
    width: calc(var(--fraction) * 100%);
    background: var(--accent);
  }

  .muted .fill {
    background: var(--text-muted);
  }

  .thumb {
    position: absolute;
    left: calc(var(--fraction) * 100%);
    width: 10px;
    height: 10px;
    margin-left: -5px;
    border-radius: 1px;
    background: var(--text);
    opacity: 0;
    transition: opacity 120ms ease;
    pointer-events: none;
  }

  .slider:not(.disabled):hover .thumb,
  .slider.dragging .thumb,
  .slider:focus-visible .thumb {
    opacity: 1;
  }

  .muted .thumb {
    opacity: 1;
  }

  /* Edge-to-edge strip: thin at rest, thicker while pointed at. */
  .strip {
    height: 10px;
    align-items: flex-start;
  }

  .strip .rail {
    height: 3px;
  }

  .strip:not(.disabled):hover .rail,
  .strip.dragging .rail {
    height: 6px;
  }

  .strip .thumb {
    display: none;
  }

  .slider:focus-visible {
    outline: none;
  }

  .slider:focus-visible .rail {
    outline: 1px solid var(--accent);
    outline-offset: 2px;
  }
</style>
