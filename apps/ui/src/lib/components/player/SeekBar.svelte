<script lang="ts">
  import { getApp } from '../../context';
  import { formatTime } from '../../format';
  import Slider from '../Slider.svelte';
  import { HOLD_MS, settle, type SentSeek } from '../../seekHold';

  /** `strip`: an edge-to-edge bar with no times, for the player bar. */
  let { variant = 'full' }: { variant?: 'full' | 'strip' } = $props();

  const { engine, position, library, settings } = getApp();
  const duration = $derived(engine.current?.track.duration_ms ?? 0);
  // The plain bar shows until the waveform arrives, and for tracks that have none.
  const levels = $derived(
    variant === 'full' && settings.current.seekBar === 'waveform'
      ? (library.waveform(engine.current?.track) ?? undefined)
      : undefined,
  );

  /** Where the pointer is while dragging, shown in the times. */
  let scrubbing = $state<number | null>(null);
  /** Where the last seek went, shown until the engine reports it there. */
  let sent = $state<SentSeek | null>(null);
  $effect(() => {
    if (sent) sent = settle(sent, position.ms, Date.now());
  });
  // Gives up on a seek that never lands even if nothing else changes meanwhile.
  $effect(() => {
    if (!sent) return;
    const timer = setTimeout(() => (sent = settle(sent, position.ms, Date.now())), HOLD_MS + 1);
    return () => clearTimeout(timer);
  });
  const at = $derived(sent?.ms ?? position.ms);
  const shown = $derived(scrubbing ?? at);
</script>

<div class="seek {variant}">
  {#if variant === 'full'}<span class="time">{formatTime(shown)}</span>{/if}
  <Slider
    value={at}
    max={duration || 1}
    label="Seek"
    valueText="{formatTime(shown)} of {formatTime(duration)}"
    disabled={!duration || !engine.can('playback')}
    variant={variant === 'strip' ? 'strip' : 'normal'}
    running={engine.player?.status === 'playing'}
    {levels}
    preview
    move="seek:now"
    oninput={(v) => (scrubbing = v)}
    onchange={(v) => {
      scrubbing = null;
      sent = { ms: Math.round(v), at: Date.now() };
      engine.run({ type: 'seek', position_ms: sent.ms });
    }}
  />
  {#if variant === 'full'}<span class="time">{duration ? formatTime(duration) : '-:--'}</span>{/if}
</div>

<style>
  .seek {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
  }

  .time {
    flex: none;
    min-width: 3.2em;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
</style>
