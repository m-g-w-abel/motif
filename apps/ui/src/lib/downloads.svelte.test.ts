import { describe, expect, it, vi } from 'vitest';
import { Downloads, memberLabel, onlyCopies, progress } from './downloads.svelte';
import type { EngineClient } from './engine.svelte';
import type { Downloadable } from './protocol/Downloadable';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { LibraryTrack } from './protocol/LibraryTrack';
import type { TrackRef } from './protocol/TrackRef';

const album: Downloadable = { kind: 'album', album: 'Low\u001fBlue' };

function engine(members: Downloadable[] = [], folder = '/music/Motif') {
  const sent: LibraryQuery[] = [];
  const e = {
    status: 'open',
    library: { revision: 1, playlists: 0, downloads: { folder, waiting: 0, error: null } },
    pairing: null,
    can: () => true,
    query: vi.fn(async (q: LibraryQuery) => {
      sent.push(q);
      return q.type === 'downloaded' ? { type: 'downloaded', members } : { type: 'done' };
    }),
  } as unknown as EngineClient;
  return { e, sent };
}

describe('downloads', () => {
  it('fetches the members and knows which are in', async () => {
    const { e } = engine([album]);
    const downloads = new Downloads(e);
    downloads.sync();
    await vi.waitFor(() => expect(downloads.has(album)).toBe(true));
    expect(downloads.has({ kind: 'album', album: 'Low\u001fRed' })).toBe(false);
  });

  it('offers Download, then Remove download, sending the toggle each time', async () => {
    const { e, sent } = engine();
    const downloads = new Downloads(e);
    const [add] = downloads.items([album]);
    expect(add.label).toBe('Download');
    add.run();
    expect(downloads.has(album)).toBe(true);
    const [remove] = downloads.items([album]);
    expect(remove.label).toBe('Remove download');
    remove.run();
    await vi.waitFor(() => expect(downloads.has(album)).toBe(false));
    await vi.waitFor(() => expect(sent.filter((q) => q.type === 'set_downloaded')).toHaveLength(2));
    expect(sent.at(-1)).toEqual({ type: 'set_downloaded', member: album, downloaded: false });
  });

  it('removes a mixed pick only when every one is in', () => {
    const track: Downloadable = { kind: 'track', path: 'remote:k:1' };
    const downloads = new Downloads(engine([album]).e);
    downloads.members = [album];
    expect(downloads.items([album, track])[0].label).toBe('Download');
  });

  it('offers nothing where this device may not change the library', () => {
    const { e } = engine();
    e.can = () => false;
    expect(new Downloads(e).items([album])).toEqual([]);
  });

  it('marks library files in the download folder as downloaded copies', () => {
    const downloads = new Downloads(engine().e);
    expect(downloads.isCopy({ source: 'local', id: '/music/Motif/Low/Blue/01 A.opus' })).toBe(true);
    expect(downloads.isCopy({ source: 'local', id: '/music/Motifs/a.opus' })).toBe(false);
    expect(downloads.isCopy({ source: 'local', id: 'remote:k:1' })).toBe(false);
  });

  it('names members by what they are', () => {
    expect(memberLabel(album)).toEqual({ title: 'Blue', line: 'Album · Low' });
    expect(memberLabel({ kind: 'artist', name: 'Low' })).toEqual({ title: 'Low', line: 'Artist' });
  });

  it('counts how many of a member\'s songs are here', () => {
    const rows = ['remote:k:1', '/music/Motif/a.opus', '/music/b.flac'].map((id) => ({ track: { source: 'local', id } as TrackRef }));
    expect(progress(rows)).toEqual({ here: 2, total: 3 });
  });

  it('finds the copies no linked engine holds any more', () => {
    const row = (id: string, devices: string[]) => ({ track: { source: 'local', id }, devices }) as unknown as LibraryTrack;
    const rows = [row('/music/Motif/a.opus', []), row('/music/Motif/b.opus', ['desk']), row('/music/c.flac', []), row('/music/Motif/d.opus', ['gone'])];
    const downloads = new Downloads(engine().e);
    const only = onlyCopies(rows, (t) => downloads.isCopy(t), new Set(['desk']));
    expect(only.map((r) => r.track.id)).toEqual(['/music/Motif/a.opus', '/music/Motif/d.opus']);
  });

  it('asks before removing the only copy of a song, and keeps it when told no', async () => {
    const { e, sent } = engine([album]);
    const copy = { track: { source: 'local', id: '/music/Motif/Low/Blue/1.opus', title: 'A' }, devices: [], track_no: 1 };
    (e as unknown as { query: unknown }).query = vi.fn(async (q: LibraryQuery) => {
      sent.push(q);
      if (q.type === 'album_tracks') return { type: 'tracks', total: 1, offset: 0, tracks: [copy] };
      return q.type === 'downloaded' ? { type: 'downloaded', members: [album] } : { type: 'done' };
    });
    const confirm = vi.fn(async () => false);
    const downloads = new Downloads(e, confirm);
    downloads.members = [album];
    await downloads.remove([album]);
    expect(confirm).toHaveBeenCalledOnce();
    expect(downloads.has(album)).toBe(true);
    expect(sent.some((q) => q.type === 'set_downloaded')).toBe(false);
    confirm.mockResolvedValue(true);
    await downloads.remove([album]);
    expect(downloads.has(album)).toBe(false);
  });

  it('shows a member downloading until all its songs are here', async () => {
    const { e } = engine([album]);
    const rows = [{ track: { source: 'local', id: 'remote:k:1' }, devices: ['desk'] }, { track: { source: 'local', id: '/music/Motif/2.opus' }, devices: [] }];
    (e as unknown as { query: unknown }).query = vi.fn(async (q: LibraryQuery) =>
      q.type === 'album_tracks' ? { type: 'tracks', total: 2, offset: 0, tracks: rows } : q.type === 'downloaded' ? { type: 'downloaded', members: [album] } : { type: 'done' },
    );
    const downloads = new Downloads(e);
    expect(downloads.state(album)).toEqual({ kind: 'off' });
    downloads.sync();
    await vi.waitFor(() => expect(downloads.state(album)).toEqual({ kind: 'downloading', share: 0.5 }));
  });

  it("shows each song's own progress while it waits or comes, and nothing once it is here", async () => {
    const { e } = engine([album]);
    const remote = (id: number, hash: string) => ({ source: 'local', id: `remote:k:${id}`, keys: { audio_hash: hash } }) as unknown as TrackRef;
    const copy = { source: 'local', id: '/music/Motif/2.opus', keys: { audio_hash: 'h2' } } as unknown as TrackRef;
    const rows = [
      { track: remote(1, 'h1'), devices: ['desk'] },
      { track: remote(3, 'h3'), devices: ['desk'] },
      { track: copy, devices: [] },
    ];
    (e as unknown as { query: unknown }).query = vi.fn(async (q: LibraryQuery) =>
      q.type === 'album_tracks' ? { type: 'tracks', total: 3, offset: 0, tracks: rows } : q.type === 'downloaded' ? { type: 'downloaded', members: [album] } : { type: 'done' },
    );
    e.library!.downloads.fetching = { source: 'remote:k:1', percent: 40 };
    const downloads = new Downloads(e);
    downloads.sync();
    await vi.waitFor(() => expect(downloads.song(remote(3, 'h3'))).toEqual({ kind: 'downloading', share: 0 }));
    expect(downloads.song(remote(1, 'h1'))).toEqual({ kind: 'downloading', share: 0.4 });
    expect(downloads.song(copy)).toEqual({ kind: 'here' });
    // A playlist lists the linked engine's row of a song with a copy here.
    expect(downloads.song(remote(2, 'h2'))).toEqual({ kind: 'here' });
    expect(downloads.song(remote(9, 'h9'))).toBeNull();
  });

  it("moves each song's ring only forward, and only the song downloading now fills", async () => {
    const { e } = engine([album]);
    const remote = (id: number) => ({ source: 'local', id: `remote:k:${id}`, keys: { audio_hash: `h${id}` } }) as unknown as TrackRef;
    const rows = [1, 2].map((id) => ({ track: remote(id), devices: ['desk'] }));
    (e as unknown as { query: unknown }).query = vi.fn(async (q: LibraryQuery) =>
      q.type === 'album_tracks' ? { type: 'tracks', total: 2, offset: 0, tracks: rows } : q.type === 'downloaded' ? { type: 'downloaded', members: [album], excluded: [] } : { type: 'done' },
    );
    const downloads = new Downloads(e);
    downloads.sync();
    await vi.waitFor(() => expect(downloads.song(remote(1))).toEqual({ kind: 'downloading', share: 0 }));
    const fetching = (source: string, percent: number) => (e.library!.downloads.fetching = { source, percent });
    fetching('remote:k:1', 99);
    expect(downloads.song(remote(1))).toEqual({ kind: 'downloading', share: 0.99 });
    expect(downloads.song(remote(2))).toEqual({ kind: 'downloading', share: 0 });
    // Saved, not scanned in yet: it holds rather than going back.
    e.library!.downloads.fetching = null;
    expect(downloads.song(remote(1))).toEqual({ kind: 'downloading', share: 0.99 });
    fetching('remote:k:2', 30);
    expect(downloads.song(remote(1))).toEqual({ kind: 'downloading', share: 0.99 });
    expect(downloads.song(remote(2))).toEqual({ kind: 'downloading', share: 0.3 });
  });

  it('offers Remove download on a song here as a download from any member, and forgets one taken out', async () => {
    const { e } = engine([album]);
    const copy = { source: 'local', id: '/music/Motif/Low/Blue/01 One.opus', keys: { audio_hash: 'h1' } } as unknown as TrackRef;
    const linked = { source: 'local', id: 'remote:k:1', keys: { audio_hash: 'h1' } } as unknown as TrackRef;
    const out = { source: 'local', id: 'remote:k:2', keys: { audio_hash: 'h2' } } as unknown as TrackRef;
    const rows = [
      { track: copy, devices: [] },
      { track: out, devices: ['desk'] },
    ];
    (e as unknown as { query: unknown }).query = vi.fn(async (q: LibraryQuery) =>
      q.type === 'album_tracks'
        ? { type: 'tracks', total: 2, offset: 0, tracks: rows }
        : q.type === 'downloaded'
          ? { type: 'downloaded', members: [album], excluded: ['remote:k:2'] }
          : { type: 'done' },
    );
    const downloads = new Downloads(e);
    downloads.sync();
    await vi.waitFor(() => expect(downloads.state(album)).toEqual({ kind: 'done' }));
    expect(downloads.songItems([copy])[0].label).toBe('Remove download');
    expect(downloads.songItems([linked])[0].label).toBe('Remove download');
    expect(downloads.song(out)).toBeNull();
    expect(downloads.songItems([out])[0].label).toBe('Download');
  });
});
