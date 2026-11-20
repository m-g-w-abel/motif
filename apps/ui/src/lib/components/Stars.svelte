<script lang="ts">
  import { MAX_HALVES, nextRating, ratingLabel } from '../ratings';

  /** Five stars in half steps: a press on a star's left half rates half a star less than its right; the same rating again clears it. */
  let { rating, size = 18, set }: { rating: number | null; size?: number; set: (rating: number | null) => void } = $props();
  let hover = $state<number | null>(null);
  const shown = $derived(hover ?? rating ?? 0);
</script>

<div class="stars" role="group" aria-label="Rating" style:--size="{size}px" onpointerleave={() => (hover = null)}>
  {#each [1, 2, 3, 4, 5] as star (star)}
    <span class="star" aria-hidden="true">
      <svg viewBox="0 0 24 24" width={size} height={size}><path d="M12 3l2.7 5.8 6.3.7-4.7 4.3 1.3 6.3L12 17l-5.6 3.1 1.3-6.3L3 9.5l6.3-.7z" /></svg>
      <svg class="fill" class:half={shown === star * 2 - 1} class:full={shown >= star * 2} viewBox="0 0 24 24" width={size} height={size}>
        <path d="M12 3l2.7 5.8 6.3.7-4.7 4.3 1.3 6.3L12 17l-5.6 3.1 1.3-6.3L3 9.5l6.3-.7z" />
      </svg>
    </span>
  {/each}
  <!-- Ten buttons over the stars, one per half: keyboard and screen readers get them, a pointer gets the nearest. -->
  <div class="halves">
    {#each { length: MAX_HALVES } as _, i (i)}
      {@const halves = i + 1}
      <button
        aria-pressed={rating === halves}
        aria-label={ratingLabel(halves)}
        title={ratingLabel(halves)}
        onpointerenter={() => (hover = halves)}
        onclick={() => set(nextRating(rating, halves))}
      ></button>
    {/each}
  </div>
</div>

<style>
  .stars {
    position: relative;
    display: inline-flex;
    flex: none;
    color: var(--accent);
  }

  .star {
    position: relative;
    display: block;
    width: var(--size);
    height: var(--size);
  }

  svg {
    display: block;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linejoin: round;
    opacity: 0.45;
  }

  .fill {
    position: absolute;
    inset: 0;
    fill: currentColor;
    opacity: 1;
    clip-path: inset(0 100% 0 0);
  }

  .fill.half {
    clip-path: inset(0 50% 0 0);
  }

  .fill.full {
    clip-path: none;
  }

  .halves {
    position: absolute;
    inset: 0;
    display: flex;
  }

  .halves button {
    flex: 1;
    min-width: 0;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }

  .halves button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
