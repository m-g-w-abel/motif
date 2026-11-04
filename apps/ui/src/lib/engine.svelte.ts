import { untrack } from 'svelte';
import { COMMAND_SCOPES, queryScope } from './access';
import type { ClientMessage } from './protocol/ClientMessage';
import type { Command } from './protocol/Command';
import type { IntegrationChanges } from './protocol/IntegrationChanges';
import type { IntegrationSettings } from './protocol/IntegrationSettings';
import type { LibraryQuery } from './protocol/LibraryQuery';
import type { LibraryStatus } from './protocol/LibraryStatus';
import type { PairingCommand } from './protocol/PairingCommand';
import type { PairingState } from './protocol/PairingState';
import type { Scope } from './protocol/Scope';
import type { QueryResult } from './protocol/QueryResult';
import type { PlayerState } from './protocol/PlayerState';
import type { QueueItem } from './protocol/QueueItem';
import type { ServerMessage } from './protocol/ServerMessage';
import { toasts } from './toast.svelte';

export type ConnectionStatus = 'connecting' | 'open' | 'closed';

const PROTOCOL_VERSION = 9;
const MAX_BACKOFF_MS = 5000;

const TOKEN_KEY = 'pairing-token';

/** This device's token from pairing (`POST /pair`), if it has one; loopback pages need none. */
export function savedToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function saveToken(token: string | null) {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    // Lasts until reload.
  }
}

/** `url` with this device's token, for requests that can't send headers (`<img src>`, WebSocket). */
export function withToken(url: string): string {
  const token = savedToken();
  return token ? `${url}${url.includes('?') ? '&' : '?'}token=${encodeURIComponent(token)}` : url;
}

/** Where the engine lives: the embedded one under Tauri, else the page's host. */
async function engineUrl(): Promise<string> {
  if ('__TAURI_INTERNALS__' in window) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<string>('engine_url');
  }
  const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
  return `${scheme}://${location.host}/ws`;
}

/**
 * A live connection to an engine. Reconnects on its own; state is reactive.
 */
export class EngineClient {
  status = $state<ConnectionStatus>('connecting');
  player = $state<PlayerState | null>(null);
  /** Tracks added to play before the context resumes. Raw (not deeply reactive): only ever replaced. */
  queue = $state.raw<QueueItem[]>([]);
  /** The context's entries from `offset` (all of a small one, a window of a big one). */
  context = $state.raw<{ offset: number; items: QueueItem[] }>({ offset: 0, items: [] });
  /** `null` until the engine reports it; engines without a library never do. */
  library = $state<LibraryStatus | null>(null);
  /** The host's integration settings; `null` until it sends them. */
  integrations = $state<IntegrationSettings | null>(null);
  /** What this connection may do; `null` until the engine says, when everything shows. */
  scopes = $state.raw<Scope[] | null>(null);
  /** Paired devices and pairing in progress; only Full clients get it. */
  pairing = $state.raw<PairingState | null>(null);
  /** The engine refused this device: never paired, or its token was revoked. */
  unpaired = $state(false);
  /** Where the engine serves HTTP (artwork), e.g. `http://127.0.0.1:7700`. */
  httpBase = $state('');
  /** Engine clock minus ours, so snapshot times (`sampled_at_ms`) can be read on our clock. */
  clockOffset = $state(0);
  error = $state<string | null>(null);

  /** The entry that is playing or paused, if any. */
  current: QueueItem | null = $derived(this.player?.current ?? null);

  #socket: WebSocket | null = null;
  #nextId = 1;
  #pending = new Map<number, { resolve: () => void; reject: (e: Error) => void }>();
  #queries = new Map<number, { resolve: (r: QueryResult) => void; reject: (e: Error) => void }>();
  #backoff = 250;
  #closed = false;
  /** Visualisers listening for spectrum bands; the engine sends them only while there are any. */
  #bands = new Set<(bands: number[]) => void>();

  constructor() {
    void this.#connect();
  }

  /** Sends a command; resolves once the engine accepts it. */
  send(command: Command): Promise<void> {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('not connected to the engine'));
    }
    // Controls are hidden where the scope is missing; gestures (double-click to play) are only ignored.
    if (!this.can(COMMAND_SCOPES[command.type])) return Promise.resolve();
    this.#noteClear(command.type);
    const id = this.#nextId++;
    const message: ClientMessage = { id, command };
    socket.send(JSON.stringify(message));
    return new Promise((resolve, reject) => this.#pending.set(id, { resolve, reject }));
  }

  /** Changes the host's integration settings given in `changes`; shows failures in the UI. */
  setIntegrations(changes: IntegrationChanges): void {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      this.error = 'not connected to the engine';
      return;
    }
    const id = this.#nextId++;
    const message: ClientMessage = { id, changes };
    socket.send(JSON.stringify(message));
    new Promise<void>((resolve, reject) => this.#pending.set(id, { resolve, reject })).catch((e: Error) => (this.error = e.message));
  }

  /** Whether this connection may do what needs `scope`. */
  can(scope: Scope): boolean {
    return this.scopes?.includes(scope) ?? true;
  }

  /** Sends a pairing command; shows failures in the UI. */
  pair(pairing: PairingCommand): void {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      this.error = 'not connected to the engine';
      return;
    }
    const id = this.#nextId++;
    const message: ClientMessage = { id, pairing };
    socket.send(JSON.stringify(message));
    new Promise<void>((resolve, reject) => this.#pending.set(id, { resolve, reject })).catch((e: Error) => (this.error = e.message));
  }

  /** Asks the library a question. */
  query(query: LibraryQuery): Promise<QueryResult> {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('not connected to the engine'));
    }
    const scope = queryScope(query);
    if (scope && !this.can(scope)) return Promise.resolve({ type: 'done' });
    this.#noteClear(query.type);
    const id = this.#nextId++;
    const message: ClientMessage = { id, query };
    socket.send(JSON.stringify(message));
    return new Promise((resolve, reject) => this.#queries.set(id, { resolve, reject }));
  }

  /**
   * Calls `listener` with each spectrum frame (32 bands, 0–255, about 30 a
   * second while playing). Returns the unsubscribe function; the engine stops
   * analysing once nobody listens.
   */
  onBands(listener: (bands: number[]) => void): () => void {
    this.#bands.add(listener);
    if (this.#bands.size === 1) this.#subscribe(true);
    return () => {
      if (this.#bands.delete(listener) && this.#bands.size === 0) this.#subscribe(false);
    };
  }

  /** Not awaited: a lost reply only means the connection dropped, and reconnecting subscribes again. */
  #subscribe(bands: boolean) {
    if (this.#socket?.readyState !== WebSocket.OPEN) return;
    const message: ClientMessage = { id: this.#nextId++, bands };
    this.#socket.send(JSON.stringify(message));
  }

  /** Like {@link send}, but shows failures in the UI instead of throwing. */
  run(command: Command): void {
    this.send(command).catch((e: Error) => (this.error = e.message));
  }

  /** Playing from a list sets a new context, which clears the queue: say so when that drops tracks. */
  #noteClear(type: string) {
    // Untracked: an effect that sends commands mustn't rerun when the queue changes.
    const n = untrack(() => this.queue.length);
    if (n && (type === 'set_context' || type === 'play' || type === 'play_album')) {
      toasts.show(`Queue cleared (${n} ${n === 1 ? 'track' : 'tracks'})`);
    }
  }

  close() {
    this.#closed = true;
    this.#socket?.close();
  }

  async #connect() {
    this.status = 'connecting';
    let socket: WebSocket;
    try {
      const url = await engineUrl();
      this.httpBase = url.replace(/^ws/, 'http').replace(/\/ws$/, '');
      socket = new WebSocket(withToken(url));
    } catch (e) {
      this.error = String(e);
      this.#retry();
      return;
    }
    this.#socket = socket;
    let opened = false;

    socket.onopen = () => {
      opened = true;
      this.status = 'open';
      this.error = null;
      this.#backoff = 250;
      if (this.#bands.size) this.#subscribe(true);
    };
    socket.onmessage = (e) => this.#handle(JSON.parse(e.data) as ServerMessage);
    socket.onclose = () => {
      this.status = 'closed';
      for (const { reject } of [...this.#pending.values(), ...this.#queries.values()]) {
        reject(new Error('connection closed'));
      }
      this.#pending.clear();
      this.#queries.clear();
      // A refused WebSocket says nothing about why: ask over HTTP whether it was the token.
      if (!opened) void this.#checkPaired();
      else this.#retry();
    };
  }

  async #checkPaired() {
    try {
      const response = await fetch(withToken(`${this.httpBase}/art/probe`));
      if (response.status === 401) {
        this.status = 'closed';
        this.unpaired = true;
        return;
      }
    } catch {
      // Unreachable: try again as usual.
    }
    this.#retry();
  }

  #retry() {
    if (this.#closed) return;
    setTimeout(() => void this.#connect(), this.#backoff);
    this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS);
  }

  #handle(message: ServerMessage) {
    switch (message.type) {
      case 'hello':
        // shortcut: ignores network latency (ms on a LAN), measure round trips when listen-along needs tighter sync (phase 7).
        this.clockOffset = message.now_ms - Date.now();
        if (message.protocol_version !== PROTOCOL_VERSION) {
          this.error = `engine speaks protocol v${message.protocol_version}, this UI expects v${PROTOCOL_VERSION}`;
        }
        break;
      case 'response': {
        const pending = this.#pending.get(message.id);
        this.#pending.delete(message.id);
        if (message.error === null) pending?.resolve();
        else if (pending) pending.reject(new Error(message.error));
        else this.error = message.error;
        break;
      }
      case 'query_result': {
        const pending = this.#queries.get(message.id);
        this.#queries.delete(message.id);
        if (message.result) pending?.resolve(message.result);
        else pending?.reject(new Error(message.error ?? 'query failed'));
        break;
      }
      case 'event':
        if (message.event.type === 'state') {
          this.player = message.event.state;
        } else if (message.event.type === 'queue') {
          this.queue = message.event.items;
        } else if (message.event.type === 'context') {
          this.context = { offset: message.event.offset, items: message.event.items };
        } else if (message.event.type === 'library') {
          this.library = message.event.status;
        } else if (message.event.type === 'integrations') {
          this.integrations = message.event.settings;
        } else if (message.event.type === 'access') {
          this.scopes = message.event.scopes;
        } else if (message.event.type === 'pairing') {
          this.pairing = message.event.state;
        } else if (message.event.type === 'bands') {
          for (const listener of this.#bands) listener(message.event.bands);
        }
        break;
    }
  }
}

/**
 * Playback position extrapolated between engine snapshots, so the UI shows
 * progress without the engine streaming updates. It ticks once per second of
 * playback (enough for time labels); progress bars step a pixel at a time in
 * between on their own (see Slider's `running`), so nothing runs every frame.
 */
export class PlaybackPosition {
  /** Our wall clock (`Date.now()`) at the last tick. */
  #now = $state(Date.now());
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  get ms(): number {
    return this.at(this.#now);
  }

  /** The position at wall clock time `now`, for timers that need it to the millisecond, not the second. */
  at(now: number): number {
    const player = this.engine.player;
    if (!player) return 0;
    if (player.status !== 'playing') return player.position_ms;
    // Snapshots are taken when something changes, so one can be old: a client
    // that connects mid-track gets the one from when the track started.
    const position = player.position_ms + Math.max(0, now + this.engine.clockOffset - player.sampled_at_ms);
    return Math.min(position, this.engine.current?.track.duration_ms ?? Infinity);
  }

  /** Call from an `$effect`; ticks on each whole second of playback while playing. */
  track(): (() => void) | undefined {
    if (this.engine.player?.status !== 'playing') return;
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      this.#now = Date.now();
      timer = setTimeout(tick, 1000 - (this.ms % 1000));
    };
    tick();
    return () => clearTimeout(timer);
  }
}
