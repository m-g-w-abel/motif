<script lang="ts" module>
  const paths = {
    play: 'M7 4.5v15L20 12z',
    shuffle: 'M16 3h5v5M4 20 21 3M21 16v5h-5M15 15l6 6M4 4l5 5',
    pause: 'M6.5 4.5h4v15h-4zM13.5 4.5h4v15h-4z',
    next: 'M5 4.5v15L15.5 12zM16.5 4.5h2.5v15h-2.5z',
    previous: 'M19 4.5v15L8.5 12zM5 4.5h2.5v15H5z',
    repeat: 'M17 2l4 4-4 4M3 11V9a3 3 0 0 1 3-3h15M7 22l-4-4 4-4m14-1v2a3 3 0 0 1-3 3H3',
    volume: 'M11 5 6 9H2v6h4l5 4V5Zm4.5 3.5a5 5 0 0 1 0 7M18.5 5.5a9 9 0 0 1 0 13',
    close: 'M6 6l12 12M18 6 6 18',
    check: 'M5 12.5l4.5 4.5L19 7.5',
    minimize: 'M5 18h14',
    maximize: 'M5 5h14v14H5z',
    plus: 'M12 5v14M5 12h14',
    music: 'M9 18V5l12-2v13M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Zm12-2a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z',
    library: 'M4 4v16M9 4v16M14 4l5 16',
    'chevron-up': 'M6 15l6-6 6 6',
    'chevron-down': 'M6 9l6 6 6-6',
    'chevron-left': 'M15 6l-6 6 6 6',
    'chevron-right': 'M9 6l6 6-6 6',
    list: 'M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01',
    sort: 'M3 6h18M6 12h12M10 18h4',
    grid: 'M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z',
    split: 'M3 3h18v18H3zM10 3v18M14 8h4M14 12h4',
    info: 'M3 3h18v18H3zM15 3v18M18 8v.01M17 12h2',
    columns: 'M3 3h18v18H3zM3 9h18M9 9v12M15 3v18',
    menu: 'M4 6h16M4 12h16M4 18h16',
    more: 'M5 12h.01M12 12h.01M19 12h.01',
    settings: 'M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z',
    grip: 'M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01',
    search: 'M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13ZM15.5 15.5 20 20',
    folder: 'M3 6h6l2 2h10v11H3z',
    copy: 'M9 9h11v11H9zM5 15V4h11',
    lyrics: 'M8 7h8M5 12h14M8 17h8',
    'queue-add': 'M3 6h13M3 12h13M3 18h8M18 14v7M14.5 17.5h7',
    heart: 'M12 20.5s-8-4.6-8-10.3A4.4 4.4 0 0 1 12 7.6a4.4 4.4 0 0 1 8 2.6c0 5.7-8 10.3-8 10.3Z',
    retry: 'M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5',
    cloud: 'M7 19h10a4.5 4.5 0 0 0 .4-9A6 6 0 0 0 6 10.6 4.3 4.3 0 0 0 7 19Z',
    'playlist-add': 'M9 18V6l10-2v5M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0ZM18 13v8M14 17h8',
  } as const;

  // Material Symbols (Outlined, weight 400), Apache License 2.0, from github.com/google/material-design-icons:
  // filled shapes on a 960-unit box, whose 80-unit lines draw as the 2-pixel strokes above do at 24.
  const symbols = {
    'cloud-download':
      'M260-160q-91 0-155.5-63T40-377q0-78 47-139t123-78q17-72 85-137t145-65q33 0 56.5 23.5T520-716v242l64-62 56 56-160 160-160-160 56-56 64 62v-242q-76 14-118 73.5T280-520h-20q-58 0-99 41t-41 99q0 58 41 99t99 41h480q42 0 71-29t29-71q0-42-29-71t-71-29h-60v-80q0-48-22-89.5T600-680v-93q74 35 117 103.5T760-520q69 8 114.5 59.5T920-340q0 75-52.5 127.5T740-160H260Zm220-358Z',
    'cloud-done':
      'm414-280 226-226-58-58-169 169-84-84-57 57 142 142ZM260-160q-91 0-155.5-63T40-377q0-78 47-139t123-78q25-92 100-149t170-57q117 0 198.5 81.5T760-520q69 8 114.5 59.5T920-340q0 75-52.5 127.5T740-160H260Zm0-80h480q42 0 71-29t29-71q0-42-29-71t-71-29h-60v-80q0-83-58.5-141.5T480-720q-83 0-141.5 58.5T280-520h-20q-58 0-99 41t-41 99q0 58 41 99t99 41Zm220-240Z',
  } as const;

  const filled = new Set<IconName>(['play', 'pause', 'next', 'previous']);

  export type IconName = keyof typeof paths | keyof typeof symbols;
  const isSymbol = (name: IconName): name is keyof typeof symbols => name in symbols;
</script>

<script lang="ts">
  let { name, size = 20, solid = false }: { name: IconName; size?: number; /** Filled in, outline kept (a heart that is on). */ solid?: boolean } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  aria-hidden="true"
  fill={solid || filled.has(name) || isSymbol(name) ? 'currentColor' : 'none'}
  stroke={filled.has(name) || isSymbol(name) ? 'none' : 'currentColor'}
  stroke-width="2"
  stroke-linecap="square"
  stroke-linejoin="miter"
>
  {#if isSymbol(name)}
    <path d={symbols[name]} transform="translate(0 24) scale(0.025)" />
  {:else}
    <path d={paths[name]} />
  {/if}
</svg>
