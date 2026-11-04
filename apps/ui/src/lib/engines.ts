/**
 * The engines this device can control, one at a time. Client settings, kept
 * in this device's storage; engines know nothing of the list.
 */
export interface SavedEngine {
  /** The engine key from pairing (`Paired.engine_key`); {@link LOCAL_ID} for this device's own. */
  id: string;
  name: string;
  /** `host:port`, tried in turn; empty means this device's own engine (embedded, or the page's host). */
  addresses: string[];
  /** This device's token for that engine; `null` where it needs none (loopback). */
  token: string | null;
}

export interface EngineList {
  engines: SavedEngine[];
  /** The id of the engine in control. */
  current: string;
}

/** This device's own engine. An entry like the others, so it can be listed elsewhere later under its key. */
export const LOCAL_ID = 'local';

const local = (token: string | null): SavedEngine => ({ id: LOCAL_ID, name: 'This device', addresses: [], token });

const isEngine = (e: unknown): e is SavedEngine => {
  const x = e as SavedEngine;
  return typeof x?.id === 'string' && typeof x.name === 'string' && Array.isArray(x.addresses) && (x.token === null || typeof x.token === 'string');
};

/** The saved list (`raw`, JSON), or a fresh one holding the token from before there was a list. */
export function loadEngines(raw: string | null, legacyToken: string | null): EngineList {
  let saved: Partial<EngineList> = {};
  try {
    saved = JSON.parse(raw ?? '{}') ?? {};
  } catch {
    // A fresh list.
  }
  const engines = Array.isArray(saved.engines) ? saved.engines.filter(isEngine) : [];
  if (!engines.some((e) => e.id === LOCAL_ID)) engines.unshift(local(raw ? null : legacyToken));
  const current = engines.some((e) => e.id === saved.current) ? saved.current! : LOCAL_ID;
  return { engines, current };
}

export const currentEngine = (list: EngineList): SavedEngine => list.engines.find((e) => e.id === list.current)!;

/** Adds `engine`, or replaces the entry with its id (pairing again gives a new token). */
export function addEngine(list: EngineList, engine: SavedEngine): EngineList {
  if (!list.engines.some((e) => e.id === engine.id)) return { ...list, engines: [...list.engines, engine] };
  return { ...list, engines: list.engines.map((e) => (e.id === engine.id ? engine : e)) };
}

export function setToken(list: EngineList, id: string, token: string | null): EngineList {
  return { ...list, engines: list.engines.map((e) => (e.id === id ? { ...e, token } : e)) };
}

export function switchTo(list: EngineList, id: string): EngineList {
  return list.engines.some((e) => e.id === id) ? { ...list, current: id } : list;
}

/** Forgets an engine; this device's own stays. */
export function removeEngine(list: EngineList, id: string): EngineList {
  if (id === LOCAL_ID) return list;
  return { engines: list.engines.filter((e) => e.id !== id), current: list.current === id ? LOCAL_ID : list.current };
}

/** Where to connect on the `attempt`th try; `own` is this device's engine. */
export function socketUrl(engine: SavedEngine, attempt: number, own: string): string {
  const { addresses } = engine;
  return addresses.length ? `ws://${addresses[attempt % addresses.length]}/ws` : own;
}
