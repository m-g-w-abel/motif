import { describe, expect, it, vi } from 'vitest';
import { Downloads, memberLabel, progress } from './downloads.svelte';
import type { EngineClient } from './engine.svelte';
import type { Downloadable } from './protocol/Downloadable';
import type { LibraryQuery } from './protocol/LibraryQuery';
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
    expect(downloads.has(album)).toBe(false);
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
});
