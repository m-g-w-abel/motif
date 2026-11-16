<script lang="ts">
  import { flushSync } from 'svelte';
  import { SLOP } from '../../gestures';
  import { released, stepTo } from '../../sheet.svelte';
  import { getApp, setViewState } from '../../context';
  import { mapped } from '../../gestures';
  import { change, type Change } from '../../motion/change';
  import { css, onto } from '../../motion/fit';
  import { rootZoom } from '../../motion/mover';
  import { still, Transition, type Span } from '../../motion/transition';
  import Icon from '../Icon.svelte';
  import Artwork from './Artwork.svelte';
  import Transport from './Transport.svelte';
  import Scrollbar from '../Scrollbar.svelte';
  import ArtworkWidget from '../widgets/Artwork.svelte';
  import Controls from '../widgets/Controls.svelte';
  import Lyrics from '../widgets/Lyrics.svelte';
  import Queue from '../widgets/Queue.svelte';
  import TrackInfo from '../widgets/TrackInfo.svelte';
  import { trackTitle } from '../../format';
  import { marquee } from '../../marquee';

  const { engine, layout, settings, sheet, views } = getApp();
  let body = $state<HTMLElement>();
  let section = $state<HTMLElement>();
  let backdrop = $state<HTMLElement>();
  let tint = $state<HTMLElement>();
  let leaving = $state<HTMLElement>();
  setViewState({ source: 'library', layoutId: 'now-playing', browse: layout.browseState('now-playing') });

  /**
   * The open/close transition's stretches (docs/ANIMATION.md): the bar's
   * own parts go first (`leave`) and the shared ones travel (`move`); on
   * phones the names under the artwork wait for it to rise past them
   * (`under`), and the album's line, which starts under the title in the
   * bar and ends above it, sets off earlier (`album`). The sheet's other
   * parts rise once the movers are nearly there (`enter`), the queue from
   * below (`queue`), and the bar's view buttons fall away (`fall`).
   */
  const spans = (phone: boolean): Record<string, Span> => ({
    leave: [0, 0.1],
    move: [0.1, 0.85],
    enter: [0.6, 1],
    under: phone ? [0.4, 0.85] : [0.1, 0.85],
    album: phone ? [0.2, 0.85] : [0.1, 0.85],
    queue: [0.5, 1],
    fall: [0, 0.5],
  });
  /** The background grows until the movers arrive, so they never poke out of it. */
  const GROW: Span = [0, 0.85];

  /** The step change under way (phones). */
  let stepping: Change | null = null;

  /**
   * Bar (0) to sheet (1): shared parts move from the bar to their places
   * here, the bar's other parts go and the sheet's come, and the
   * background grows out of the bar's box, its corners opening out to the
   * screen's and its colour turning to the sheet's. Without a player to
   * grow out of, or with reduced motion, the whole sheet fades.
   */
  function toggle(): Transition | null {
    if (!section || !backdrop || !tint || !leaving) return null;
    stepping?.end();
    const origin = sheet.origin;
    if (!origin || still()) {
      flushSync();
      return new Transition([{ el: section, frames: [{ opacity: 0 }, { opacity: 1 }] }]);
    }
    const t = change(section, () => flushSync(), { from: origin, spans: spans(views.phone), under: leaving });
    const z = rootZoom();
    const box = (r: DOMRect) => ({ left: r.left / z, top: r.top / z, width: r.width / z, height: r.height / z });
    const [o, b] = [box(origin.getBoundingClientRect()), box(backdrop.getBoundingClientRect())];
    const grow = onto({ ...b, left: 0, top: 0 }, { ...o, left: o.left - b.left, top: o.top - b.top });
    // Phones: the bar is a rounded card inset from the edges; its corners, undone by the scale.
    const r = views.phone ? parseFloat(getComputedStyle(origin).borderTopLeftRadius) || 0 : 0;
    t.add({ el: backdrop, frames: [{ transform: css(grow), borderRadius: `${r / grow.sx}px / ${r / grow.sy}px` }, { transform: 'none', borderRadius: '0px' }], span: GROW });
    t.add({ el: tint, frames: [{ opacity: 1 }, { opacity: 0 }], span: GROW });
    return t;
  }

  /**
   * Phones: a step change (now playing, half or full queue, lyrics) by
   * `apply`, as a transition between the two layouts. `undo` puts the step
   * back if a finger lets it spring back.
   */
  function step(apply: () => void, undo?: () => void): Change | null {
    if (!section || !leaving || !views.phone || still()) {
      apply();
      return null;
    }
    stepping?.end();
    const t = change(
      section,
      () => {
        apply();
        flushSync();
      },
      {
        under: leaving,
        ended: (landed) => {
          if (stepping === t) stepping = null;
          if (landed || !undo) return;
          undo();
          flushSync();
        },
      },
    );
    stepping = t;
    return t;
  }

  sheet.motion = { toggle, step: (apply) => void step(apply)?.play(1) };

  /**
   * Phones: a drag on one of the sheet's handles. Once it moves, the step
   * change it heads for (up towards the queue, back towards now playing)
   * follows the finger, then finishes or springs back on letting go, as
   * `released` decides. A tap calls `onTap`; a drag that would close the
   * sheet is the sheet's own (`closing`, else on letting go).
   */
  function stepDrag(e: PointerEvent, onTap: () => void, closing?: () => void) {
    if (e.button !== 0) return;
    if (!views.phone || still()) return closing ? closing() : sheet.handle(e, onTap);
    const y0 = e.clientY;
    const from = sheet.step;
    let way: 'up' | 'back' | null = null;
    let t: Change | null = null;
    const move = (ev: PointerEvent) => {
      const dy = ev.clientY - y0;
      if (!way) {
        if (Math.abs(dy) < SLOP) return;
        way = dy < 0 ? 'up' : 'back';
        const to = stepTo(from, way);
        if (to === 'closed' && closing) {
          stop();
          return closing();
        }
        if (!to || to === 'closed' || !sheet.rest) return;
        t = step(
          () => (sheet.step = to),
          () => (sheet.step = from),
        );
      }
      t?.seek((way === 'up' ? -dy : dy) / t.distance);
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', end);
      window.removeEventListener('pointercancel', end);
    };
    const end = (ev: PointerEvent) => {
      stop();
      const what = ev.type === 'pointercancel' ? null : released(ev.clientY - y0);
      if (t) return void t.release(what === way);
      if (what === 'tap') onTap();
      else if (what === 'up') sheet.up();
      else if (what === 'back') sheet.back();
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', end);
    window.addEventListener('pointercancel', end);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && sheet.open) sheet.collapse();
  }

  /**
   * Phones: a drag on now playing closes it (down, the sheet's drag) or
   * brings up the queue (up, following the finger like the handle); the seek
   * bar keeps its own, and the artwork unless its drag is remapped away from this.
   */
  function dragNow(e: PointerEvent) {
    if (!views.phone || sheet.step !== 'now') return;
    const own = mapped(settings.current.gestures, 'artwork')['drag-vertical'] !== 'close';
    if ((e.target as Element).closest(own ? '.art, [role="slider"], .more' : '[role="slider"], .more')) return;
    stepDrag(e, () => {}, () => sheet.drag(e, 'sheet', undefined, true));
  }

  /** The keyboard's press on a handle; pointers go through `sheet.handle`. */
  const keyed = (e: MouseEvent, then: () => void) => e.detail === 0 && then();
  const album = $derived(engine.current?.track.album ?? '');
  let coverWidth = $state(0);
</script>

<svelte:window onkeydown={onKey} />

<!--
  Grows out of the player bar (or floating player): the background grows from
  its box and the artwork, title, play button and seek bar move from their places
  in it (`toggle`). Parts declare their roles (`data-move`, `data-enter`,
  `data-span`; docs/ANIMATION.md). Built from the same widgets layouts use.
-->
<section
  bind:this={section}
  class="sheet"
  data-step={sheet.step}
  class:open={sheet.open || sheet.moving}
  aria-label="Now playing"
  aria-hidden={!sheet.open}
  inert={!sheet.open}
>
  <div class="backdrop" bind:this={backdrop}><div class="tint" bind:this={tint}></div></div>
  <!-- Copies of the parts a change takes away, going behind the rest. -->
  <div class="leaving" bind:this={leaving} aria-hidden="true"></div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header data-enter="rise" onpointerdown={(e) => sheet.drag(e, 'sheet')}>
    <span class="handle"></span>
    <button
      class="icon-button lyrics-toggle"
      class:active={sheet.lyrics}
      aria-label="Lyrics"
      aria-pressed={sheet.lyrics}
      title={sheet.lyrics ? 'Show artwork' : 'Show lyrics'}
      onclick={() => sheet.toggleLyrics()}
    >
      <Icon name="lyrics" size={18} />
    </button>
    <button class="icon-button" aria-label="Collapse" title="Collapse (Esc)" onclick={() => sheet.collapse()}>
      <Icon name="chevron-down" size={20} />
    </button>
  </header>

  <!-- Phones, full queue and lyrics: the playing track in a strip whose drag or tap steps back. -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="mini" data-enter="above" onpointerdown={(e) => !(e.target as Element).closest('.transport') && stepDrag(e, () => sheet.back())}>
    <div class="mini-row">
      <Artwork track={engine.current?.track} size={48} move="cover:now" />
      <span class="mini-names">
        <b data-move="title:now">{engine.current ? trackTitle(engine.current.track) : 'Nothing playing'}</b>
        <span data-move="artist:now">{engine.current?.track.artist ?? ''}</span>
      </span>
      <Transport compact size={22} />
    </div>
    <button class="step" aria-label={sheet.lyrics ? 'Back to now playing' : 'Show less of the queue'} onclick={(e) => keyed(e, () => sheet.back())}></button>
  </div>

  <div class="body" bind:this={body}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="now" onpointerdown={dragNow}>
      <!-- Phones, half queue: the cover, full width, behind the title. -->
      {#if views.phone && sheet.step === 'half'}
        <div class="cover-bg" aria-hidden="true" bind:clientWidth={coverWidth}>
          {#if coverWidth}<Artwork track={engine.current?.track} size={coverWidth} move="cover:now" />{/if}
        </div>
        <!-- Apart from the cover, which travels as a copy: the shade fades over it as it arrives. -->
        <span class="shade" data-enter="fade"></span>
      {/if}
      <div class="art" class:lyrics={sheet.lyrics}>
        {#if sheet.lyrics}<div class="lyrics-slot" data-enter="rise"><Lyrics /></div>{/if}
        <!-- Kept laid out, unseen, while the lyrics show: drawn afresh (rebuilt, or from display: none),
             the carousel's cards fade in (its @starting-style), which blinked the cover on closing them. -->
        <div class="keep" hidden={sheet.lyrics}><ArtworkWidget expands={false} playing hidden={sheet.step !== 'now'} /></div>
      </div>
      <div class="details" data-enter="rise">
        <p class="album-title" data-enter="fade" use:marquee={sheet.rest ? (engine.current?.track.id ?? null) : null}>
          <span class="marquee" data-move="album:now" data-span="album">{album}</span>
        </p>
        <TrackInfo style="hero" playing>
          {#snippet aside()}
            <button class="lyrics-button" aria-label="Lyrics" aria-pressed={sheet.lyrics} title="Show lyrics" onclick={() => sheet.toggleLyrics()}>
              <Icon name="lyrics" size={22} />
            </button>
          {/snippet}
        </TrackInfo>
        <Controls />
        <!-- Phones: the handle at the bottom; a tap or drag up shows the queue. -->
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="more" data-enter="fade" onpointerdown={(e) => stepDrag(e, () => sheet.up())}>
          <button class="step" aria-label="Show the queue" onclick={(e) => keyed(e, () => sheet.up())}></button>
        </div>
      </div>
    </div>
    <div class="queue-slot" data-enter="below" data-span="queue">
      <button
        class="step"
        aria-label="Show all of the queue"
        onpointerdown={(e) => stepDrag(e, () => sheet.up())}
        onclick={(e) => keyed(e, () => sheet.up())}
      ></button>
      <Queue collapsible={!views.phone} nowPlaying={views.phone} />
    </div>
  </div>
  <Scrollbar target={body} />
</section>

<style>
  .sheet {
    position: absolute;
    inset: 0;
    z-index: 30;
    display: flex;
    flex-direction: column;
    /* Shown while open or on its way (its transition draws the rest). */
    visibility: hidden;
  }

  .sheet.open {
    visibility: visible;
  }

  .backdrop {
    position: absolute;
    inset: 0;
    z-index: -1;
    overflow: hidden;
    background: var(--bg);
    transform-origin: 0 0;
  }

  /* The bar's colour, fading into the sheet's as it grows. */
  .tint {
    position: absolute;
    inset: 0;
    background: var(--surface);
    opacity: 0;
  }

  /* Not clipped: the bar's parts can lie outside the sheet (desktop). */
  .leaving {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }

  /* Let the artwork travel to the bar instead of being clipped by its box. */
  .art :global(.artwork) {
    overflow: visible;
    background: none;
  }

  header {
    position: relative;
    display: flex;
    justify-content: flex-end;
    align-items: center;
    height: 40px;
    padding: 0 var(--space-2);
    flex: none;
    touch-action: none;
    cursor: grab;
  }

  /* Phone parts. */
  .mini,
  .album-title,
  .lyrics-button,
  .more,
  .queue-slot > .step {
    display: none;
  }

  .handle {
    position: absolute;
    left: 50%;
    top: 8px;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--border);
  }

  .body {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  /* Artwork beside the details; together they take whatever the queue leaves. */
  .now {
    flex: 1 1 auto;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(300px, 460px) minmax(0, 0.4fr);
    align-items: center;
    gap: var(--space-6);
    padding: 0 var(--space-6) var(--space-6);
  }

  .art {
    height: 100%;
    min-height: 0;
  }

  /* Lyrics are as large as the space allows, not square like the artwork. */
  @media (min-width: 701px) {
    .now:has(.art.lyrics) {
      grid-template-columns: minmax(0, 1.5fr) minmax(300px, 460px) minmax(0, 0.2fr);
    }
  }

  .art.lyrics {
    align-self: stretch;
  }

  /* Lays the artwork out as if it were the art's own child. */
  .keep {
    display: contents;
  }

  .art.lyrics {
    position: relative;
  }

  .keep[hidden] {
    display: block;
    position: absolute;
    inset: 0;
    visibility: hidden;
    pointer-events: none;
  }

  .art.lyrics :global(.lyrics) {
    background: none;
  }

  .lyrics-slot {
    height: 100%;
  }

  .lyrics-toggle.active {
    color: var(--accent);
  }

  .details {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  /* The queue sits underneath: a header when folded, ~40% of the height when open. */
  .queue-slot {
    flex: none;
    border-top: 1px solid var(--divider);
  }

  .queue-slot:has(:global(.queue.open)) {
    flex: 0 1 40%;
    min-height: 160px;
  }

  /* Phones: cover the whole screen; artwork, details and queue stack and scroll. */
  @media (max-width: 700px) {
    .sheet {
      position: fixed;
      z-index: 45;
      padding-top: env(safe-area-inset-top);
      padding-bottom: env(safe-area-inset-bottom);
    }

    /* Doesn't scroll: the queue and lyrics scroll inside it. */
    .body {
      position: relative;
      overflow: hidden;
    }

    /* Fills the screen, so the handle sits at the bottom; long titles shrink the cover instead of pushing it off. */
    .now {
      flex: 1 1 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
      align-items: stretch;
      gap: var(--space-4);
      padding: 0 var(--space-4) var(--space-2);
    }

    /* The screen's width (the carousel keeps a small margin), plus room for its dots below. */
    .art {
      flex: 0 1 min(calc(100vw + 28px), 66vh);
      min-height: 40vw;
      height: auto;
      margin-inline: calc(-1 * var(--space-4));
    }

    .details {
      flex: 1 0 auto;
    }

    /* The handles step the sheet; Android's back and swipes close it. */
    header {
      display: none;
    }

    .now {
      padding-top: var(--space-2);
    }

    [data-step='now'] .now {
      touch-action: none;
    }

    .step {
      display: block;
      width: 100%;
      height: 28px;
      padding: 0;
      border: 0;
      background: none;
      cursor: grab;
      touch-action: none;
    }

    .step::before {
      content: '';
      display: block;
      width: 44px;
      height: 5px;
      margin: 0 auto;
      border-radius: 3px;
      background: var(--border);
    }

    /* One line each, so the controls never move between tracks; what doesn't fit scrolls (`marquee.ts`). */
    .album-title {
      display: flex;
      height: calc(1.3em + 0.4em);
      margin: 0;
      font-size: 1.5em;
      font-weight: 700;
      line-height: 1.3;
      overflow: hidden;
      white-space: nowrap;
      /* Room in the clipping box for ascenders and descenders, taken back outside it. */
      padding-block: 0.2em;
      margin-block: -0.2em;
    }

    /* Centred while it fits; auto margins drop to 0 when it overflows, so it starts at the left. */
    .album-title .marquee {
      flex: none;
      margin-inline: auto;
    }

    /* Clipping one-line boxes (`marquee.ts`), with room for ascenders and descenders (a 'g') taken back outside them. */
    .details :global(.hero :is(h1, .hero-artist)) {
      overflow: hidden;
      white-space: nowrap;
      padding-block: 0.2em;
    }

    .details :global(.hero h1) {
      margin-block: -0.2em;
    }

    .details :global(.hero .hero-artist) {
      margin-block: calc(var(--space-1) - 0.2em) -0.2em;
    }

    .details :global(.marquee) {
      display: inline-block;
    }

    .details :global(.hero-album) {
      min-height: 1.6em;
    }

    /* The controls sit low, just above the handle; the space goes between them and the names. */
    .details > :global(.controls) {
      margin-top: auto;
    }

    .lyrics-button {
      display: grid;
      place-items: center;
      flex: none;
      width: 46px;
      height: 46px;
      padding: 0;
      border: 0;
      border-radius: var(--radius-lg, 12px);
      background: var(--text);
      color: var(--bg);
      cursor: pointer;
    }

    /* The album's name sits above the title; under it, the format badge alone. */
    .details :global(.album-name) {
      display: none;
    }

    .details :global(.hero-album) {
      justify-content: center;
    }

    .details :global(.hero .format) {
      padding: 2px var(--space-3);
      border: 0;
      border-radius: 999px;
      background: var(--text);
      color: var(--bg);
      text-transform: uppercase;
    }

    .details :global(.hero .rates) {
      display: inline;
    }

    .more {
      display: block;
      padding: var(--space-3) 0;
      touch-action: none;
    }

    .queue-slot {
      display: none;
      border-top: 0;
    }

    /* Half and full: the queue fills what is left and scrolls on its own; the rest stays put. */

    [data-step='half'] .now {
      flex: none;
    }

    .queue-slot:has(:global(.queue.open)) {
      flex: 1 1 0;
      min-height: 0;
      flex-direction: column;
    }

    .queue-slot :global(.queue) {
      flex: 1;
      min-height: 0;
    }

    /* Half: the cover fills the top behind the title, the queue below the controls. */
    [data-step='half'] .now {
      position: relative;
      padding-top: calc(min(100vw, 62vh) - 7em);
    }

    /* Kept laid out (not display: none) so the carousel needn't rebuild on the way back. */
    [data-step='half'] .art {
      position: absolute;
      inset: 0 0 auto;
      height: min(100vw, 62vh);
      visibility: hidden;
    }

    /* Square and full width, its bottom where the 62vh band ends: a taller cover runs off the top. */
    .cover-bg {
      position: absolute;
      left: 0;
      right: 0;
      top: calc(min(100vw, 62vh) - 100vw);
      z-index: -1;
    }

    .cover-bg :global(.art) {
      border-radius: 0;
    }

    /* Over the cover, the same square. */
    .shade {
      position: absolute;
      left: 0;
      right: 0;
      top: calc(min(100vw, 62vh) - 100vw);
      height: 100vw;
      z-index: -1;
      background: linear-gradient(transparent 45%, var(--bg));
    }

    [data-step='half'] :is(.album-title, .more),
    [data-step='half'] .details :global(.hero-album) {
      display: none;
    }

    [data-step='half'] .queue-slot,
    [data-step='full'] .queue-slot {
      display: flex;
    }

    [data-step='half'] .queue-slot > .step {
      display: block;
    }

    /* Full queue and lyrics: the strip on top, the rest below it. */
    :is([data-step='full'], [data-step='lyrics']) .mini {
      display: block;
      flex: none;
      padding-top: var(--space-3);
      background: var(--surface);
      touch-action: none;
    }

    .mini-row {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: 0 var(--space-3) var(--space-2) var(--space-4);
    }

    .mini-names {
      flex: 1;
      min-width: 0;
      display: flex;
      flex-direction: column;
    }

    .mini-names > * {
      overflow-wrap: anywhere;
    }

    .mini-names span {
      color: var(--text-muted);
    }

    [data-step='full'] .now {
      display: none;
    }

    [data-step='lyrics'] .now {
      flex: 1;
      min-height: 0;
      padding: 0;
    }

    [data-step='lyrics'] .details {
      display: none;
    }

    /* A flex box, as WebKit leaves a percentage height unresolved in a flex item. */
    [data-step='lyrics'] .art.lyrics {
      flex: 1 1 0;
      min-height: 0;
      display: flex;
      flex-direction: column;
    }

    [data-step='lyrics'] .lyrics-slot {
      flex: 1;
      min-height: 0;
      height: auto;
      display: flex;
      flex-direction: column;
    }

    [data-step='lyrics'] .art.lyrics :global(.lyrics) {
      flex: 1;
      min-height: 0;
      height: auto;
    }
  }
</style>
