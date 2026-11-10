<script lang="ts">
  import { untrack } from 'svelte';
  import { getApp, getViewState } from '../../context';
  import { contextMenu, type MenuItem } from '../../contextMenu.svelte';
  import { formatTime } from '../../format';
  import type { LibraryTracks, Shown } from '../../library.svelte';
  import { queueTracks } from '../../menus';
  import type { LibraryAlbum } from '../../protocol/LibraryAlbum';
  import type { LibraryArtist } from '../../protocol/LibraryArtist';
  import type { LibraryFolder } from '../../protocol/LibraryFolder';
  import { trackListData } from '../../sources.svelte';
  import { viewOptions } from '../../viewOptions';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';
  import AlbumGrid from '../widgets/AlbumGrid.svelte';
  import SongList from '../widgets/SongList.svelte';

  // The Search view: a field, filter chips, a toolbar acting on the results, and the results.
  const { engine, library, views } = getApp();
  const area = getViewState();

  const FILTERS = ['All', 'Songs', 'Albums', 'Artists', 'Album Artists', 'Folders', 'Playlists'] as const;
  let filter = $state<(typeof FILTERS)[number]>('All');
  let selecting = $state(false);

  // The field shows what is typed at once; the results follow when typing pauses.
  let query = $state(untrack(() => area.browse.search));
  let typing: ReturnType<typeof setTimeout> | undefined;
  function search(text: string, now = false) {
    query = text;
    clearTimeout(typing);
    if (now) area.browse.search = text.trim();
    else typing = setTimeout(() => (area.browse.search = text.trim()), 250);
  }
  const searched = $derived(area.browse.search);
  let field = $state<HTMLInputElement>();
  $effect(() => field?.focus());

  // A shortcut's limit: lists of names are drawn whole, so they stop here.
  const NAMES = 200;
  const ALBUM_SORT = viewOptions({}, 'albums').sort ?? [];
  let artists = $state.raw<LibraryArtist[]>([]);
  let folders = $state.raw<LibraryFolder[]>([]);
  let best = $state.raw<{ artist: LibraryArtist; albums: LibraryAlbum[] } | null>(null);
  let others = $state.raw<LibraryAlbum[]>([]);
  /** Albums matching the search, but for the best match's. */
  let otherCount = $state(0);
  // Only the latest search's answers are shown.
  let asked = 0;
  $effect(() => {
    const text = searched || null;
    const kind = filter;
    void engine.library?.revision;
    const ask = ++asked;
    const fail = (e: Error) => (engine.error = e.message);
    if (!text) return;
    if (kind === 'Artists' || kind === 'Album Artists') {
      engine
        .query({ type: 'artists', search: text, album_artists: kind === 'Album Artists', offset: 0, limit: NAMES })
        .then((r) => ask === asked && r.type === 'artists' && (artists = r.artists))
        .catch(fail);
    } else if (kind === 'Folders') {
      engine
        .query({ type: 'folders', search: text, offset: 0, limit: NAMES })
        .then((r) => ask === asked && r.type === 'folders' && (folders = r.folders))
        .catch(fail);
    } else if (kind === 'All') {
      void allResults(text, ask).catch(fail);
    }
  });

  /** All's best match (the album artist named most like the search, with their albums) and the other albums. */
  async function allResults(text: string, ask: number) {
    const [found, matching] = await Promise.all([
      engine.query({ type: 'artists', search: text, album_artists: true, offset: 0, limit: NAMES }),
      engine.query({ type: 'albums', search: text, sort: ALBUM_SORT, offset: 0, limit: 7 }),
    ]);
    if (found.type !== 'artists' || matching.type !== 'albums') return;
    // The artist named just so, else the one with the most songs.
    const lower = text.toLowerCase();
    const artist =
      found.artists.find((a) => a.name.toLowerCase() === lower) ??
      found.artists.reduce<LibraryArtist | undefined>((top, a) => (!top || a.tracks > top.tracks ? a : top), undefined);
    const theirs = artist
      ? await engine.query({ type: 'albums', search: null, artist: artist.name, sort: ALBUM_SORT, offset: 0, limit: NAMES })
      : null;
    if (ask !== asked) return;
    const albums = theirs?.type === 'albums' ? theirs.albums : [];
    best = artist ? { artist, albums } : null;
    others = matching.albums.filter((a) => a.artist !== artist?.name).slice(0, 6);
    // Every album of theirs matches too: the search found them by name.
    otherCount = matching.total - albums.length;
  }

  const shownList: Shown<LibraryTracks> = {};
  const songs = $derived(trackListData(engine, library, 'library', searched || null, [], shownList));
  let songList = $state<SongList>();
  let grid = $state<AlbumGrid>();

  const openAlbum = (key: string, title: string, art?: string | null) => views.open({ kind: 'album', key, title, art });
  /** An artist's row: search for them, to see their albums and songs. */
  function pickArtist(name: string) {
    filter = 'All';
    search(name, true);
  }
  function playFolder(path: string) {
    engine.query({ type: 'play_folder', path, start: 0 }).catch((e: Error) => (engine.error = e.message));
  }
  const folderName = (path: string) => path.split(/[/\\]/).pop() || path;
  const folderParent = (path: string) => path.slice(0, path.length - folderName(path).length - 1);

  const grids = $derived(filter === 'Albums' || filter === 'Playlists');
  // shortcut: play/shuffle on Artists and Folders play the songs matching the search, not only theirs; upgrade with an artist filter on `play`.
  async function playAll(shuffle = false) {
    if (grids) return grid?.playEverything(shuffle);
    if (songList) return songList.playEverything(shuffle);
    try {
      const found = await engine.query({ type: 'tracks', search: searched, sort: [], offset: 0, limit: 0 });
      if (found.type !== 'tracks' || !found.total) return;
      await engine.send({ type: 'set_shuffle', on: shuffle });
      await engine.query({ type: 'play', search: searched, sort: [], start: shuffle ? Math.floor(Math.random() * found.total) : 0 });
    } catch (e) {
      engine.error = (e as Error).message;
    }
  }

  /** The ⋮ menu: queue the matching songs. */
  function openMore(e: MouseEvent) {
    const matching = async () => {
      const found = await engine.query({ type: 'tracks', search: searched, sort: [], offset: 0, limit: songs.count });
      return found.type === 'tracks' ? found.tracks.map((t) => t.track) : [];
    };
    const items: MenuItem[] = engine.can('queue')
      ? [
          { label: 'Queue songs next', run: () => queueTracks(engine, matching(), true) },
          { label: 'Add songs to queue', run: () => queueTracks(engine, matching()) },
        ]
      : [];
    const button = e.currentTarget as HTMLElement;
    const box = button.getBoundingClientRect();
    if (items.length) contextMenu.show(items, box.right - 200, box.bottom + 4, button);
  }
  const songsHere = $derived(filter === 'All' || filter === 'Songs');
</script>

{#snippet albumRow(album: LibraryAlbum)}
  <li>
    <button class="album" onclick={() => openAlbum(album.key, album.title, album.art)}>
      <span class="cover"><Artwork track={undefined} art={album.art} title={album.title} /></span>
      <span class="lines">
        <span class="title">{album.title || 'Unknown album'}</span>
        <span class="artist">{album.artist || 'Unknown artist'}</span>
        <span class="meta">
          {album.tracks}
          {album.tracks === 1 ? 'song' : 'songs'} · {formatTime(album.duration_ms)}{album.year ? ` · ${album.year}` : ''}
        </span>
      </span>
    </button>
  </li>
{/snippet}

{#snippet allHeader()}
  {#if best}
    <div class="best">
      <span class="kind">artist</span>
      <button class="name" onclick={() => pickArtist(best!.artist.name)}>{best.artist.name}</button>
    </div>
    <ul class="albums">
      {#each best.albums as album (album.key)}{@render albumRow(album)}{/each}
    </ul>
  {/if}
  {#if others.length}
    <h2 class="sub">Albums</h2>
    <ul class="albums">
      {#each others as album (album.key)}{@render albumRow(album)}{/each}
    </ul>
    {#if otherCount > others.length}
      <button class="more-link" onclick={() => (filter = 'Albums')}>All {otherCount} albums</button>
    {/if}
  {/if}
  {#if songs.count}<h2 class="sub">Songs</h2>{/if}
{/snippet}

{#snippet nameRow(name: string, line: string, art: string | null, onclick: () => void)}
  <li>
    <button class="name-row" {onclick}>
      <span class="cover small">
        {#if art}<Artwork track={undefined} {art} title={name} />{:else}<span class="initial">{name[0]?.toUpperCase()}</span>{/if}
      </span>
      <span class="lines">
        <span class="title">{name}</span>
        <span class="artist">{line}</span>
      </span>
    </button>
  </li>
{/snippet}

<div class="search-view">
  <div class="head">
    <label class="field">
      <Icon name="search" size={20} />
      <input
        bind:this={field}
        type="search"
        placeholder="Search your library"
        aria-label="Search the library"
        autocomplete="off"
        value={query}
        oninput={(e) => search(e.currentTarget.value)}
        onkeydown={(e) => e.key === 'Enter' && search(query, true)}
      />
      {#if query}
        <button
          class="clear"
          aria-label="Clear"
          title="Clear"
          onclick={() => {
            search('', true);
            field?.focus();
          }}><Icon name="close" size={18} /></button
        >
      {/if}
    </label>
    <div class="chips" role="radiogroup" aria-label="Show">
      {#each FILTERS as name (name)}
        <button
          role="radio"
          aria-checked={filter === name}
          class:on={filter === name}
          onclick={() => {
            filter = name;
            selecting = false;
          }}>{name}</button
        >
      {/each}
    </div>
    <div class="actions">
      {#if engine.can('playback')}
        <button class="tool" aria-label="Shuffle" title="Shuffle" disabled={!searched} onclick={() => playAll(true)}>
          <Icon name="shuffle" size={18} />
        </button>
        <button class="tool" aria-label="Play" title="Play" disabled={!searched} onclick={() => playAll()}>
          <Icon name="play" size={18} />
        </button>
      {/if}
      {#if songsHere}
        <button class="tool text" class:active={selecting} aria-pressed={selecting} disabled={!searched} onclick={() => (selecting = !selecting)}>Select</button>
      {/if}
      <span class="grow"></span>
      <button class="tool" aria-label="Search options" title="More" aria-haspopup="menu" disabled={!searched} onclick={openMore}>
        <Icon name="more" size={18} />
      </button>
    </div>
  </div>

  <div class="results">
    {#if !searched}
      <p class="empty">Search songs, albums, artists, folders and playlists.</p>
    {:else if songsHere}
      {#key filter}
        <SongList
          bind:this={songList}
          bind:selecting
          data={songs}
          mode="covers"
          header={filter === 'All' ? allHeader : undefined}
          openAlbum={(key, title) => openAlbum(key, title)}
        />
      {/key}
    {:else if grids}
      {#key filter}
        <AlbumGrid
          bind:this={grid}
          cardSize={150}
          mode="covers"
          items={filter === 'Playlists' ? 'playlists' : 'albums'}
          sort={filter === 'Albums' ? ALBUM_SORT : undefined}
          onopen={(key, title, art) =>
            filter === 'Playlists' ? views.open({ kind: 'playlist', id: key, name: title }) : openAlbum(key, title, art)}
        />
      {/key}
    {:else}
      <ul class="names">
        {#if filter === 'Folders'}
          {#each folders as folder (folder.path)}
            {@render nameRow(
              folderName(folder.path),
              `${folderParent(folder.path)} · ${folder.tracks} ${folder.tracks === 1 ? 'song' : 'songs'} · ${formatTime(folder.duration_ms)}`,
              folder.art,
              () => playFolder(folder.path),
            )}
          {:else}
            <li class="empty">No folders match “{searched}”.</li>
          {/each}
        {:else}
          {#each artists as artist (artist.name)}
            {@render nameRow(
              artist.name,
              `${artist.albums} ${artist.albums === 1 ? 'album' : 'albums'} · ${artist.tracks} ${artist.tracks === 1 ? 'song' : 'songs'}`,
              artist.art,
              () => pickArtist(artist.name),
            )}
          {:else}
            <li class="empty">No artists match “{searched}”.</li>
          {/each}
        {/if}
      </ul>
    {/if}
  </div>
</div>

<style>
  .search-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .head {
    padding: var(--space-3) var(--space-3) var(--space-2);
    flex: none;
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    height: 48px;
    padding: 0 var(--space-2) 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text-muted);
  }

  .field input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: 1.15em;
  }

  /* The field's own ✕ instead of the browser's. */
  .field input::-webkit-search-cancel-button {
    display: none;
  }

  .clear {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--text);
    cursor: pointer;
  }

  .chips {
    display: flex;
    gap: var(--space-2);
    margin: var(--space-3) calc(-1 * var(--space-3)) 0;
    padding: 0 var(--space-3);
    overflow-x: auto;
    scrollbar-width: none;
  }

  .chips button {
    height: 34px;
    padding: 0 var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: var(--surface-2);
    color: var(--text);
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }

  .chips button.on {
    border-color: var(--text);
    background: var(--text);
    color: var(--bg);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .tool {
    display: grid;
    place-items: center;
    width: 44px;
    height: 36px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    background: none;
    color: var(--text);
    cursor: pointer;
  }

  .tool.text {
    width: auto;
    padding: 0 var(--space-3);
    font-weight: 600;
  }

  .tool:hover:not(:disabled),
  .tool.active {
    background: var(--hover);
  }

  .tool:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .grow {
    flex: 1;
  }

  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .empty {
    margin: 0;
    padding: var(--space-4) var(--space-3);
    color: var(--text-muted);
    list-style: none;
  }

  .best {
    display: flex;
    align-items: baseline;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-3) 0;
  }

  .kind {
    color: var(--text-muted);
    font-size: 1.1em;
    font-weight: 600;
  }

  .best .name {
    min-width: 0;
    padding: 0;
    border: 0;
    overflow: hidden;
    background: none;
    color: var(--text);
    font-size: 1.8em;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .sub {
    margin: var(--space-3) var(--space-3) var(--space-1);
    color: var(--text-muted);
    font-size: 0.9em;
    font-weight: 700;
    text-transform: uppercase;
  }

  .albums,
  .names {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .album,
  .name-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }

  .album:hover,
  .name-row:hover,
  .more-link:hover {
    background: var(--hover);
  }

  .cover {
    width: 84px;
    flex: none;
  }

  .cover.small {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--surface-2);
  }

  .cover :global(.art) {
    width: 100%;
    border-radius: var(--radius-sm);
  }

  .initial {
    font-size: 1.2em;
    font-weight: 700;
  }

  .lines {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .lines > span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .album .title {
    font-size: 1.15em;
    font-weight: 700;
  }

  .name-row .title {
    font-weight: 600;
  }

  .artist {
    font-weight: 600;
  }

  .name-row .artist,
  .meta {
    color: var(--text-muted);
    font-size: 0.9em;
    font-weight: normal;
  }

  .more-link {
    display: block;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--accent);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }
</style>
