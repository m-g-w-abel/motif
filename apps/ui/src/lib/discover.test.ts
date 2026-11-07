import { describe, expect, it } from 'vitest';
import { openInAppLink, parsePairLink } from './discover';

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

  it('reads the link the pairing page hands to the app', () => {
    expect(parsePairLink('motif://192.168.1.5:7700/pair?code=ABCD2345&id=k3y')).toEqual({ address: '192.168.1.5:7700', code: 'ABCD2345', id: 'k3y' });
    expect(parsePairLink('motif://[fe80::1]/pair?code=C&id=k')?.address).toBe('[fe80::1]:80');
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

describe('open in app link', () => {
  it('opens the same pairing link in Motif, falling back to the page', () => {
    const page = 'http://192.168.1.5:7700/pair?code=ABCD2345&id=k3y';
    const link = openInAppLink(page);
    expect(link).toBe(
      'intent://192.168.1.5:7700/pair?code=ABCD2345&id=k3y#Intent;scheme=motif;package=io.github.mgwabel.motif;' +
        'S.browser_fallback_url=http%3A%2F%2F192.168.1.5%3A7700%2Fpair%3Fcode%3DABCD2345%26id%3Dk3y;end',
    );
    // What Android hands the app is motif:// plus the part after intent://.
    expect(parsePairLink(`motif:${link.slice('intent:'.length, link.indexOf('#'))}`)).toEqual(parsePairLink(page));
  });
});
