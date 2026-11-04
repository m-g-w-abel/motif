import { describe, expect, it } from 'vitest';
import { treeUriToPath } from './folders';

const tree = (id: string) => `content://com.android.externalstorage.documents/tree/${encodeURIComponent(id)}`;

describe('treeUriToPath', () => {
  it('maps the phone storage to /storage/emulated/0', () => {
    expect(treeUriToPath(tree('primary:Music'))).toBe('/storage/emulated/0/Music');
    expect(treeUriToPath(tree('primary:Music/Rock & Roll'))).toBe('/storage/emulated/0/Music/Rock & Roll');
    expect(treeUriToPath(tree('primary:'))).toBe('/storage/emulated/0');
  });

  it('maps an SD card or USB drive to /storage/<volume id>', () => {
    expect(treeUriToPath(tree('1234-ABCD:Music'))).toBe('/storage/1234-ABCD/Music');
    expect(treeUriToPath(tree('1234-ABCD:'))).toBe('/storage/1234-ABCD');
  });

  it('ignores a document part after the tree', () => {
    expect(treeUriToPath(`${tree('primary:Music')}/document/${encodeURIComponent('primary:Music/A')}`)).toBe(
      '/storage/emulated/0/Music',
    );
  });

  it('rejects folders that have no plain path', () => {
    expect(treeUriToPath('content://com.android.providers.downloads.documents/tree/downloads')).toBeNull();
    expect(treeUriToPath('content://com.google.android.apps.docs.storage/tree/abc')).toBeNull();
    expect(treeUriToPath(tree('../etc:x'))).toBeNull();
    expect(treeUriToPath(tree('primary:Music/../../..'))).toBeNull();
    expect(treeUriToPath('/storage/emulated/0/Music')).toBeNull();
  });
});
