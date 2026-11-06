import { describe, expect, it } from 'vitest';
import { parsePairLink } from './discover';

describe('pairing link', () => {
  it('reads the address, code and engine key', () => {
    expect(parsePairLink('http://192.168.1.5:7700/pair?code=ABCD2345&id=k3y')).toEqual({ address: '192.168.1.5:7700', code: 'ABCD2345', id: 'k3y' });
  });

  it('keeps IPv6 brackets and fills in the default port', () => {
    expect(parsePairLink('http://[fe80::1]:7700/pair?code=C&id=k')?.address).toBe('[fe80::1]:7700');
    expect(parsePairLink('http://10.0.0.2/pair?code=C&id=k')?.address).toBe('10.0.0.2:80');
  });

  it('ignores surrounding whitespace from the scanner', () => {
    expect(parsePairLink(' http://10.0.0.2:7700/pair?code=C&id=k\n')?.code).toBe('C');
  });

  it('rejects anything that is not a pairing link', () => {
    for (const text of [
      'hello',
      'https://example.com/',
      'http://10.0.0.2:7700/',
      'http://10.0.0.2:7700/pairing?code=C&id=k',
      'https://10.0.0.2:7700/pair?code=C&id=k',
      'http://10.0.0.2:7700/pair?id=k',
      'http://10.0.0.2:7700/pair?code=C',
      'http://10.0.0.2:7700/pair?code=&id=k',
      'http://user@10.0.0.2:7700/pair?code=C&id=k',
    ]) {
      expect(parsePairLink(text), text).toBeNull();
    }
  });
});
