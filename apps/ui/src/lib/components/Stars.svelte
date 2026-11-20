<script lang="ts">
  import { nextRating, STARS, starsLabel, starsOf } from '../ratings';

  /**
   * Five stars, filled up to the rating and open after it; pressing one rates that many, pressing the one the
   * rating already shows clears it. `tabbable` off (table rows) leaves them out of the tab order.
   */
  let { rating, size = 18, tabbable = true, set }: { rating: number | null; size?: number; tabbable?: boolean; set: (rating: number | null) => void } = $props();
  const shown = $derived(starsOf(rating));
</script>

<div class="stars" role="group" aria-label="Rating">
  {#each { length: STARS } as _, i (i)}
    {@const n = i + 1}
    <!-- A press here is not the row's: it neither selects it nor plays it. -->
    <button
      class="star"
      class:on={n <= shown}
      aria-pressed={n === shown}
      aria-label={starsLabel(n)}
      title={starsLabel(n)}
      tabindex={tabbable ? 0 : -1}
      onpointerdown={(e) => e.stopPropagation()}
      ondblclick={(e) => e.stopPropagation()}
      onclick={(e) => {
        e.stopPropagation();
        set(nextRating(rating, n));
      }}
    >
      <svg viewBox="0 0 24 24" width={size} height={size}><path d="M12 3l2.7 5.8 6.3.7-4.7 4.3 1.3 6.3L12 17l-5.6 3.1 1.3-6.3L3 9.5l6.3-.7z" /></svg>
    </button>
  {/each}
</div>

<style>
  .stars {
    display: inline-flex;
    flex: none;
    color: var(--accent);
  }

  .star {
    display: grid;
    place-items: center;
    padding: 1px;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
  }

  svg {
    display: block;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linejoin: round;
    opacity: 0.5;
  }

  .on svg {
    fill: currentColor;
    opacity: 1;
  }

  .star:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
</style>
