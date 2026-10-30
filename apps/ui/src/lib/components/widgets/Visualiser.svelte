<script lang="ts">
  import { getApp } from '../../context';

  const { engine } = getApp();

  /** Bars fall at most this much per frame (of 255), so they sink rather than flicker. */
  const FALL = 18;

  let canvas = $state<HTMLCanvasElement>();
  let onScreen = $state(false);
  let pageVisible = $state(document.visibilityState === 'visible');
  const live = $derived(onScreen && pageVisible && engine.player?.status === 'playing');

  /** Bar heights as drawn, 0–255. */
  let shown: number[] = [];
  let frame = 0;

  function draw() {
    frame = 0;
    const c = canvas;
    const ctx = c?.getContext('2d');
    if (!c || !ctx) return;
    const { width, height } = c;
    ctx.clearRect(0, 0, width, height);
    if (!shown.length) return;
    ctx.fillStyle = getComputedStyle(c).getPropertyValue('--accent');
    // Whole device pixels for every bar and gap, so the spacing is even; the leftover is split either side.
    const n = shown.length;
    const gap = Math.max(1, Math.round((width / n) * 0.2));
    const bar = Math.max(1, Math.floor((width - gap * (n - 1)) / n));
    const left = Math.floor((width - bar * n - gap * (n - 1)) / 2);
    shown.forEach((level, i) => {
      const h = Math.round((level / 255) * height);
      if (h) ctx.fillRect(left + i * (bar + gap), height - h, bar, h);
    });
  }

  function receive(bands: number[]) {
    shown = bands.map((b, i) => Math.max(b, (shown[i] ?? 0) - FALL));
    frame ||= requestAnimationFrame(draw);
  }

  // Listen only while it can be seen and something plays; otherwise the engine does no analysis.
  $effect(() => {
    if (!live) return;
    const stop = engine.onBands(receive);
    return () => {
      stop();
      cancelAnimationFrame(frame);
      shown = [];
      draw();
    };
  });

  $effect(() => {
    const c = canvas;
    if (!c) return;
    const resize = new ResizeObserver(([entry]) => {
      const box = entry.devicePixelContentBoxSize?.[0];
      c.width = box?.inlineSize ?? Math.round(entry.contentRect.width * devicePixelRatio);
      c.height = box?.blockSize ?? Math.round(entry.contentRect.height * devicePixelRatio);
      draw();
    });
    const seen = new IntersectionObserver(([entry]) => (onScreen = entry.isIntersecting));
    resize.observe(c);
    seen.observe(c);
    return () => {
      resize.disconnect();
      seen.disconnect();
    };
  });
</script>

<svelte:document onvisibilitychange={() => (pageVisible = document.visibilityState === 'visible')} />

<!-- Spectrum bars of what is playing, drawn as frames arrive (about 30 a second) and only while playing. -->
<canvas bind:this={canvas} class="visualiser" aria-hidden="true"></canvas>

<style>
  .visualiser {
    display: block;
    width: 100%;
    height: 100%;
    min-height: 0;
    padding: var(--space-4);
    box-sizing: border-box;
  }
</style>
