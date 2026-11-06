import type { Paired } from './protocol/Paired';

/** An engine seen on the LAN (`remote.rs` in the desktop app). */
export interface FoundEngine {
  /** The engine key. */
  id: string;
  name: string;
  /** `host:port`. */
  addresses: string[];
}

/** Browsing and pairing run in the Tauri shell (desktop and Android), not in a browser. */
export const canDiscover = () => '__TAURI_INTERNALS__' in window;

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

/** The engines answering on the LAN now; takes a couple of seconds. */
export const discoverEngines = () => invoke<FoundEngine[]>('discover_engines');

/** Redeems `code` at `address`; waits for the host to approve. Rejects with a text for the user. */
export const pairEngine = (address: string, code: string, name: string) => invoke<Paired>('pair_engine', { address, code, name });

/** Android's Wi-Fi drops multicast packets unless the app holds a lock; held only while browsing. */
export function holdMulticast(on: boolean) {
  (globalThis as { MotifAndroid?: { setMulticast(on: boolean): void } }).MotifAndroid?.setMulticast(on);
}

export interface PairLink {
  /** `host:port`. */
  address: string;
  code: string;
  /** The engine key. */
  id: string;
}

/** Reads the link in a host's pairing QR (`pair_url` in the server); null for anything else. */
export function parsePairLink(text: string): PairLink | null {
  let url: URL;
  try {
    url = new URL(text.trim());
  } catch {
    return null;
  }
  const code = url.searchParams.get('code');
  const id = url.searchParams.get('id');
  if (url.protocol !== 'http:' || url.pathname !== '/pair' || url.username || !code || !id) return null;
  // URL drops the default port; saved addresses always carry one.
  return { address: url.port ? url.host : `${url.host}:80`, code, id };
}
