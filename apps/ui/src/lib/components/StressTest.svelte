<!--
  Development only (?dev-stress, with ?dev-library=50000 for a long list):
  checks the webview keeps up with the visual design. It scrolls the track
  lists and album grids continuously under a backdrop-blurred panel holding a
  blurred artwork layer and a canvas visualiser, and reports frame rate and
  the worst frame. ?dev-stress=scroll leaves the panel out, to measure plain
  scrolling. The numbers are also on `window.stress` for scripts.
-->
<script lang="ts">
  import { demoLibrary } from '../demo';
  import { placeholderArt } from '../format';

  const panel = new URLSearchParams(location.search).get('dev-stress') !== 'scroll';
  let canvas = $state<HTMLCanvasElement>();
  let fps = $state(0);
  let worst = $state(0);

  $effect(() => {
    const ctx = canvas?.getContext('2d');
    const times: number[] = [];
    let last = performance.now();
    let frame = requestAnimationFrame(function tick(now) {
      times.push(now - last);
      last = now;
      if (times.length > 120) times.shift();
      fps = Math.round(1000 / (times.reduce((a, b) => a + b, 0) / times.length));
      worst = Math.round(Math.max(...times));
      (window as unknown as { stress: object }).stress = { fps, worst, rows: document.querySelectorAll('tr.track').length };

      for (const list of document.querySelectorAll<HTMLElement>('main .tracklist .scroll, main .album-grid .scroll')) {
        list.scrollTop = list.scrollTop + list.clientHeight >= list.scrollHeight - 1 ? 0 : list.scrollTop + 40;
      }

      frame = requestAnimationFrame(tick);
      if (!canvas || !ctx) return;
      // Fake spectrum: 64 bars from layered sines.
      const { width, height } = canvas;
      ctx.clearRect(0, 0, width, height);
      ctx.fillStyle = getComputedStyle(canvas).color;
      const bars = 64;
      for (let i = 0; i < bars; i++) {
        const level = 0.5 + 0.3 * Math.sin(now / 180 + i * 0.4) + 0.2 * Math.sin(now / 67 + i * 1.7);
        const h = level * height;
        ctx.fillRect((i * width) / bars, height - h, width / bars - 2, h);
      }
    });
    return () => cancelAnimationFrame(frame);
  });
</script>

<div class="stress" class:panel aria-hidden="true">
  {#if panel}
    <div class="art" style:background={placeholderArt(demoLibrary[0])}></div>
    <canvas bind:this={canvas} width="480" height="160"></canvas>
  {/if}
  <p>{fps} fps · worst frame {worst} ms · {demoLibrary.length} tracks</p>
</div>

<style>
  .stress {
    position: fixed;
    right: 24px;
    bottom: calc(var(--bar-height) + 24px);
    z-index: 100;
    pointer-events: none;
  }

  .panel {
    width: min(520px, calc(100vw - 48px));
    padding: var(--space-4);
    overflow: hidden;
    border: 1px solid var(--border);
    background: color-mix(in srgb, var(--surface) 40%, transparent);
    backdrop-filter: blur(30px) saturate(1.4);
    color: var(--accent);
  }

  .art {
    position: absolute;
    inset: -40px;
    z-index: -1;
    opacity: 0.5;
    filter: blur(40px);
  }

  canvas {
    display: block;
    width: 100%;
    height: 160px;
  }

  p {
    margin: var(--space-2) 0 0;
    font: 600 14px var(--font-mono);
    color: var(--text);
  }
</style>
