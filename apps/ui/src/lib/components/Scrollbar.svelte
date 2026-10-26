<!--
  A square overlay scrollbar for `target`, which hides its native one (the
  `custom-scrollbar` class). Place it as a sibling of `target`: both then
  share an offset parent, so the bar can sit over the target's right edge
  without taking layout width. It shows while the area is hovered or
  scrolled and fades out a second later.

  In sorted lists, `snaps` are the scroll offsets where groups start (the
  thumb snaps to them while dragged) and `label` names the sort value at the
  top of the view, shown in a bubble beside the thumb while dragging.
-->
<script lang="ts">
  import { nearest } from '../table';

  let {
    target,
    snaps,
    label,
  }: {
    target: HTMLElement | undefined;
    snaps?: number[];
    label?: () => string | undefined;
  } = $props();

  let thumb = $state<HTMLElement>();
  /** The target's box within the shared offset parent. */
  let box = $state({ top: 0, right: 0, height: 0 });
  let size = $state(0);
  let offset = $state(0);
  let hovered = $state(false);
  let scrolling = $state(false);
  let dragging = $state(false);
  let idle: ReturnType<typeof setTimeout> | undefined;

  const minThumb = () => (matchMedia('(pointer: coarse)').matches ? 48 : 32);

  /** Sizes the thumb and places it; it moves with a transform, so scrolling never lays out the bar. */
  function update() {
    if (!target) return;
    const { scrollTop, scrollHeight, clientHeight } = target;
    const range = scrollHeight - clientHeight;
    size = range > 0 ? Math.max(minThumb(), (clientHeight * clientHeight) / scrollHeight) : 0;
    offset = range > 0 ? (scrollTop / range) * (clientHeight - size) : 0;
  }

  function measure() {
    if (!target) return;
    box = {
      top: target.offsetTop + target.clientTop,
      right: target.offsetLeft + target.clientLeft + target.clientWidth,
      height: target.clientHeight,
    };
    update();
  }

  function settle() {
    clearTimeout(idle);
    idle = setTimeout(() => (scrolling = false), 1000);
  }

  $effect(() => {
    const el = target;
    if (!el) return;
    el.classList.add('custom-scrollbar');
    const onScroll = () => {
      update();
      scrolling = true;
      settle();
    };
    const enter = () => {
      hovered = true;
      measure();
    };
    const leave = () => (hovered = false);
    el.addEventListener('scroll', onScroll, { passive: true });
    el.addEventListener('pointerenter', enter);
    el.addEventListener('pointerleave', leave);
    // The content's size changes when rows load; the parent's when panels move.
    const observer = new ResizeObserver(measure);
    for (const node of [el, el.parentElement, el.firstElementChild]) if (node) observer.observe(node);
    return () => {
      el.classList.remove('custom-scrollbar');
      el.removeEventListener('scroll', onScroll);
      el.removeEventListener('pointerenter', enter);
      el.removeEventListener('pointerleave', leave);
      observer.disconnect();
      clearTimeout(idle);
    };
  });

  function drag(e: PointerEvent) {
    if (!target || !thumb || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const el = target;
    thumb.setPointerCapture(e.pointerId);
    dragging = true;
    lastLabel = '';
    const startY = e.clientY;
    const startTop = el.scrollTop;
    const max = el.scrollHeight - el.clientHeight;
    const ratio = max / Math.max(1, el.clientHeight - size);
    // The end stays reachable even if the last group starts above it.
    const points = snaps?.length ? [...snaps.filter((s) => s < max), max] : undefined;
    const move = (ev: PointerEvent) => {
      const to = Math.min(max, Math.max(0, startTop + (ev.clientY - startY) * ratio));
      el.scrollTop = points ? nearest(points, to) : to;
    };
    const end = () => {
      thumb?.removeEventListener('pointermove', move);
      dragging = false;
      scrolling = true;
      settle();
    };
    thumb.addEventListener('pointermove', move);
    thumb.addEventListener('pointerup', end, { once: true });
    thumb.addEventListener('pointercancel', end, { once: true });
  }

  /** Clicking the track pages towards the pointer, like a native scrollbar. */
  function page(e: PointerEvent) {
    if (!target || !thumb || e.button !== 0) return;
    e.preventDefault();
    const above = e.clientY < thumb.getBoundingClientRect().top;
    target.scrollBy({ top: (above ? -0.9 : 0.9) * target.clientHeight });
  }

  // Keeps the last known value while the rows under a drag are still loading.
  let lastLabel = '';
  const bubble = $derived.by(() => {
    if (!dragging || !label) return '';
    lastLabel = label() || lastLabel;
    return lastLabel;
  });
</script>

{#if size}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="scrollbar"
    class:shown={hovered || scrolling || dragging}
    class:dragging
    style:top="{box.top}px"
    style:left="{box.right}px"
    style:height="{box.height}px"
    aria-hidden="true"
    onpointerdown={page}
    onpointerenter={() => (hovered = true)}
    onpointerleave={() => (hovered = false)}
  >
    <div
      class="thumb"
      bind:this={thumb}
      style:height="{size}px"
      style:transform="translateY({offset}px)"
      onpointerdown={drag}
    >
      {#if bubble}<span class="bubble">{bubble}</span>{/if}
    </div>
  </div>
{/if}

<style>
  .scrollbar {
    --width: 8px;
    position: absolute;
    z-index: 2;
    width: var(--width);
    margin-left: calc(-1 * var(--width));
    opacity: 0;
    transition: opacity 0.3s;
    touch-action: none;
  }

  .scrollbar.shown {
    opacity: 1;
    transition-duration: 0.1s;
  }

  .thumb {
    position: absolute;
    top: 0;
    right: 0;
    width: 100%;
    background: var(--border);
  }

  .scrollbar:hover .thumb {
    background: var(--text-muted);
  }

  .dragging .thumb,
  .dragging:hover .thumb {
    background: var(--accent);
  }

  .bubble {
    position: absolute;
    top: 50%;
    right: calc(100% + 8px);
    transform: translateY(-50%);
    min-width: 32px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-size: 15px;
    font-weight: 600;
    text-align: center;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  /* A fast-scroll handle a finger can hold. */
  @media (pointer: coarse) {
    .scrollbar {
      --width: 14px;
    }
  }
</style>
