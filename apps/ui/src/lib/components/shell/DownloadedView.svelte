<script lang="ts">
  import { getApp } from '../../context';
  import { elsewhere } from '../../devices';
  import { memberLabel, progress } from '../../downloads.svelte';
  import { NO_FILTER } from '../../library.svelte';
  import type { Downloadable } from '../../protocol/Downloadable';
  import type { LibraryTrack } from '../../protocol/LibraryTrack';
  import Icon from '../Icon.svelte';
  import Artwork from '../player/Artwork.svelte';

  // The Downloaded view: what this device keeps copies of, each with how much of it is here, and how downloading goes.
  const { engine, downloads, playlists, views } = getApp();

  interface Row {
    title: string;
    line: string;
    art: string | null;
    here: number;
    total: number;
  }

  /** What a member's row shows, looked up from the library (an artist's albums, a playlist's songs). */
  async function rowOf(m: Downloadable): Promise<Partial<Row>> {
    const songs = async (albums: string[]): Promise<LibraryTrack[]> => {
      if (!albums.length) return [];
      const r = await engine.query({ type: 'album_tracks', albums, sort: [], filter: NO_FILTER });
      return r.type === 'tracks' ? r.tracks : [];
    };
    if (m.kind === 'album') {
      const rows = await songs([m.album]);
      return { art: rows[0]?.art ?? null, ...progress(rows) };
    }
    if (m.kind === 'artist') {
      // shortcut: an artist's first 500 albums; page them if a collector has more.
      const r = await engine.query({ type: 'albums', search: null, artist: m.name, sort: [], filter: NO_FILTER, offset: 0, limit: 500 });
      const albums = r.type === 'albums' ? r.albums : [];
      const rows = await songs(albums.map((a) => a.key));
      return { art: albums[0]?.art ?? null, line: `Artist · ${albums.length} ${albums.length === 1 ? 'album' : 'albums'}`, ...progress(rows) };
    }
    if (m.kind === 'playlist') {
      const rows = (await playlists.entries(m.id)).map((e) => e.track);
      return { art: rows.find((t) => t.art)?.art ?? null, ...progress(rows) };
    }
    const r = await engine.query({ type: 'lookup', paths: [m.path] });
    const track = r.type === 'tracks' ? r.tracks[0] : undefined;
    if (!track) return {};
    // Its copy shadows the linked engine's row, which a lookup still finds: the album lists the one that wins.
    const key = `${track.album_artist ?? track.track.artist ?? ''}\u001f${track.track.album ?? ''}`;
    const shown = (await songs([key])).find((t) => t.track.title === track.track.title && t.track_no === track.track_no) ?? track;
    return { title: track.track.title ?? undefined, line: `Song · ${track.track.artist ?? 'Unknown artist'}`, art: track.art, here: elsewhere(shown) ? 0 : 1, total: 1 };
  }

  let found = $state.raw(new Map<string, Partial<Row>>());
  const idOf = (m: Downloadable) => JSON.stringify(m);
  // Again as the library changes: a download landing changes it.
  $effect(() => {
    void engine.library?.revision;
    const members = downloads.members;
    let current = true;
    void Promise.all(members.map((m) => rowOf(m).catch(() => ({})))).then((rows) => {
      if (current) found = new Map(members.map((m, i) => [idOf(m), rows[i]]));
    });
    return () => (current = false);
  });

  const rows = $derived(
    downloads.members.map((m) => {
      const playlist = m.kind === 'playlist' ? playlists.list.find((p) => p.id === m.id) : undefined;
      const label = memberLabel(m);
      const row: Row = { title: playlist?.name ?? label.title, line: label.line, art: playlist?.art ?? null, here: 0, total: 0, ...found.get(idOf(m)) };
      return { member: m, row };
    }),
  );

  function open(m: Downloadable, title: string) {
    if (m.kind === 'album') views.open({ kind: 'album', key: m.album, title });
    else if (m.kind === 'playlist') views.open({ kind: 'playlist', id: m.id, name: title });
  }

  const status = $derived(downloads.status);
  const canChange = $derived(engine.can('library'));
</script>

<div class="downloaded">
  {#if status}
    <p class="status" aria-live="polite">
      {#if status.error}<span class="error">Last download failed: {status.error}</span>
      {:else if status.waiting}Downloading · {status.waiting} {status.waiting === 1 ? 'song' : 'songs'} to go
      {:else if downloads.members.length}Everything here is downloaded.{/if}
      {#if status.folder}<span class="folder">Saved in <span class="selectable">{status.folder}</span></span>{/if}
    </p>
  {/if}
  <ul class="members">
    {#each rows as { member, row } (idOf(member))}
      <li>
        <button class="member" disabled={member.kind !== 'album' && member.kind !== 'playlist'} onclick={() => open(member, row.title)}>
          <span class="cover">
            {#if row.art}<Artwork track={undefined} art={row.art} title={row.title} />{:else}<Icon name={member.kind === 'playlist' ? 'list' : 'music'} size={18} />{/if}
          </span>
          <span class="lines">
            <span class="title">{row.title}</span>
            <span class="line">{row.line}</span>
          </span>
          {#if row.total}
            <span class="progress" class:done={row.here === row.total} title="{row.here} of {row.total} songs on this device">
              {#if row.here === row.total}<Icon name="check" size={14} />{:else}{row.here}/{row.total}{/if}
            </span>
          {/if}
        </button>
        {#if canChange}
          <button class="icon-button" aria-label="Remove download: {row.title}" title="Remove download" onclick={() => downloads.set([member], false)}>
            <Icon name="close" size={16} />
          </button>
        {/if}
      </li>
    {:else}
      <li class="empty">Nothing downloaded yet. Download songs, albums, artists or playlists from linked devices from their menus, and they play here offline.</li>
    {/each}
  </ul>
</div>

<style>
  .status {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    margin: 0;
    padding: 0 var(--space-3) var(--space-2);
    color: var(--text-muted);
  }

  .status:empty {
    display: none;
  }

  .error {
    color: var(--danger);
  }

  .folder {
    font-size: 0.88em;
  }

  .members {
    margin: 0;
    padding: 0 0 var(--space-3);
    list-style: none;
  }

  .members li {
    display: flex;
    align-items: center;
    padding-right: var(--space-2);
  }

  .member {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--space-3);
    min-width: 0;
    padding: var(--space-2) var(--space-3);
    border: 0;
    background: none;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }

  .member:disabled {
    cursor: default;
  }

  .member:hover:not(:disabled) {
    background: var(--hover);
  }

  .cover {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-muted);
    overflow: hidden;
    flex: none;
  }

  .cover :global(.art) {
    width: 100%;
  }

  .lines {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .title,
  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    font-weight: 500;
  }

  .line {
    color: var(--text-muted);
    font-size: 0.88em;
  }

  .progress {
    display: flex;
    color: var(--text-muted);
    font-size: 0.88em;
    font-variant-numeric: tabular-nums;
    flex: none;
  }

  .progress.done {
    color: var(--accent);
  }

  .empty {
    padding: var(--space-4) var(--space-3);
    color: var(--text-muted);
  }
</style>
