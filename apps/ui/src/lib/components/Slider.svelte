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
    running = false,
    levels,
    preview = false,
    oninput,
    onchange,
    move,
  }: {
    value: number;
    max: number;
    label: string;
    /** Spoken value, e.g. "1:45 of 3:07". */
    valueText?: string;
    disabled?: boolean;
    /** `strip`: a thin edge-to-edge bar that thickens on hover. */
    variant?: 'normal' | 'strip' | 'muted';
    /** The value is advancing by 1 per millisecond (playback), so keep moving between updates. */
    running?: boolean;
    /** Loudness along the range (0–255), drawn as a waveform in place of the rail. */
    levels?: number[];
    /** While a mouse hovers ahead of the value, the stretch up to it is drawn pale (e.g. where a seek would jump). */
    preview?: boolean;
    oninput?: (value: number) => void;
    onchange?: (value: number) => void;
    /** A shared element's name (`data-move`, docs/ANIMATION.md): it travels as a line, stretched across only. */
    move?: string;
  } = $props();

  let track = $state<HTMLElement>();
  /** Set while dragging, so outside updates don't fight the pointer. */
  let dragValue = $state<number | null>(null);
  /** How far playback has moved since `value` last changed (see below). */
  let advance = $state(0);
  const shown = $derived(dragValue ?? Math.min(max, value + advance));
  const fraction = $derived(max > 0 ? Math.min(1, Math.max(0, shown / max)) : 0);
  /** Where a hovering mouse points, 0 to 1; the played part covers it when it is behind the value. */
  let hover = $state(0);

  // While running, advance one pixel at a time on a timer (a few times a
  // second for a track) rather than every frame. Plain main-thread updates:
  // WebKitGTK's GPU renderer stalls for seconds on composited animations in
  // the phone layout.
  $effect(() => {
    void value; // restart from each new value
    advance = 0;
    if (!running || !track || max <= 0) return;
    const step = Math.max(16, max / Math.max(1, track.clientWidth));
    const start = performance.now();
    let timer = setTimeout(function tick() {
      advance = performance.now() - start;
      timer = setTimeout(tick, step);
    }, step);
    return () => clearTimeout(timer);
  });

  /** The waveform as one filled outline, mirrored about the middle, in a box `levels.length` wide and 2 high. */
  const wave = $derived.by(() => {
    if (!levels?.length) return '';
    // The quietest points keep a hairline, so silence still reads as part of the track.
    const h = levels.map((l) => Math.max(0.06, l / 255).toFixed(3));
    const top = h.map((v, i) => `${i} ${(1 - +v).toFixed(3)}`);
    const bottom = h.map((v, i) => `${i} ${(1 + +v).toFixed(3)}`).reverse();
    return `M${top.join('L')}L${bottom.join('L')}Z`;
  });

  function valueAt(clientX: number): number {
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width)) * max;
  }

  function onPointerDown(e: PointerEvent) {
    if (disabled || e.button !== 0 || !track) return;
    e.preventDefault();
    hover = 0;
    track.setPointerCapture(e.pointerId);
    dragValue = valueAt(e.clientX);
    oninput?.(dragValue);
  }

  function onPointerMove(e: PointerEvent) {
    if (dragValue === null) {
      if (preview && !disabled && e.pointerType === 'mouse' && max > 0) hover = valueAt(e.clientX) / max;
      return;
    }
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
  data-move={move}
  data-fit={move && 'line'}
  class="slider {variant}"
  class:disabled
  class:waveform={!!wave}
  class:dragging={dragValue !== null}
  role="slider"
  tabindex={disabled ? -1 : 0}
  aria-label={label}
  aria-valuemin={0}
  aria-valuemax={max}
  aria-valuenow={Math.round(dragValue ?? value)}
  aria-valuetext={valueText}
  aria-disabled={disabled}
  style:--fraction={fraction}
  style:--hover={hover}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  onpointerleave={() => (hover = 0)}
  onkeydown={onKey}
>
  {#if wave}
    <!-- Drawn once per track; progress moves a clip over an accent copy with transforms only. -->
    <div class="wave">
      <svg viewBox="0 0 {levels!.length - 1} 2" preserveAspectRatio="none" aria-hidden="true"><path d={wave} /></svg>
      {#if preview}
        <div class="ahead">
          <svg viewBox="0 0 {levels!.length - 1} 2" preserveAspectRatio="none" aria-hidden="true"><path d={wave} /></svg>
        </div>
      {/if}
      <div class="played">
        <svg viewBox="0 0 {levels!.length - 1} 2" preserveAspectRatio="none" aria-hidden="true"><path d={wave} /></svg>
      </div>
    </div>
  {:else}
    <div class="rail">
      {#if preview}<div class="ahead"></div>{/if}
      <div class="fill"></div>
    </div>
  {/if}
  <div class="thumbs"><div class="thumb-track"><div class="thumb"></div></div></div>
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
    position: relative;
    height: 100%;
    transform: scaleX(var(--fraction));
    transform-origin: left;
    background: var(--accent);
  }

  /* Under the fill, so only the stretch ahead of the value shows. */
  .rail .ahead {
    position: absolute;
    inset: 0;
    transform: scaleX(var(--hover));
    transform-origin: left;
    background: var(--accent);
    opacity: 0.35;
  }

  .muted .fill {
    background: var(--text-muted);
  }

  .slider.waveform {
    height: 28px;
  }

  .wave {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .wave svg {
    display: block;
    width: 100%;
    height: 100%;
    fill: var(--text-muted);
    opacity: 0.45;
  }

  /* Slides right to reveal the accent copy, which slides back by as much to stay put. */
  .played,
  .wave .ahead {
    position: absolute;
    inset: 0;
    overflow: hidden;
    transform: translateX(calc((var(--fraction) - 1) * 100%));
  }

  .played svg {
    fill: var(--accent);
    opacity: 1;
    transform: translateX(calc((1 - var(--fraction)) * 100%));
  }

  /* The same clip up to the pointer, pale, under the played copy. */
  .wave .ahead {
    transform: translateX(calc((var(--hover) - 1) * 100%));
  }

  .wave .ahead svg {
    fill: var(--accent);
    opacity: 0.4;
    transform: translateX(calc((1 - var(--hover)) * 100%));
  }

  .muted .played svg {
    fill: var(--text-muted);
  }

  /* Clips the moved track below (transforms count as overflow and would
     make the page scroll sideways), leaving room for the thumb at the ends. */
  .thumbs {
    position: absolute;
    inset: 0 -6px;
    overflow: hidden;
    pointer-events: none;
  }

  /* Full width, so translateX(100%) moves the thumb to the end. Transforms
     keep progress updates to repaints, without layout. */
  .thumb-track {
    position: absolute;
    inset: 0 6px;
    display: flex;
    align-items: center;
    transform: translateX(calc(var(--fraction) * 100%));
    pointer-events: none;
  }

  .strip .thumb-track {
    align-items: flex-start;
  }

  .thumb {
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

  /* A waveform's played edge marks the position. */
  .strip .thumb,
  .waveform .thumb {
    display: none;
  }

  .slider:focus-visible {
    outline: none;
  }

  .slider:focus-visible .rail,
  .slider:focus-visible .wave {
    outline: 2px solid var(--text);
    outline-offset: 2px;
  }
</style>
