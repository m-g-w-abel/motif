import type { ClientMessage } from './protocol/ClientMessage';
import type { Command } from './protocol/Command';
import type { PlayerState } from './protocol/PlayerState';
import type { QueueItem } from './protocol/QueueItem';
import type { ServerMessage } from './protocol/ServerMessage';

export type ConnectionStatus = 'connecting' | 'open' | 'closed';

const PROTOCOL_VERSION = 2;
const MAX_BACKOFF_MS = 5000;

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
  /** Raw (not deeply reactive): it can hold thousands of tracks and is only ever replaced. */
  queue = $state.raw<QueueItem[]>([]);
  /** `performance.now()` when `player` arrived, for position extrapolation. */
  receivedAt = $state(0);
  error = $state<string | null>(null);

  /** The queue entry that is playing or paused, if any. */
  current: QueueItem | null = $derived(
    this.player && this.player.current !== null ? (this.queue[this.player.current] ?? null) : null,
  );

  #socket: WebSocket | null = null;
  #nextId = 1;
  #pending = new Map<number, { resolve: () => void; reject: (e: Error) => void }>();
  #backoff = 250;
  #closed = false;

  constructor() {
    void this.#connect();
  }

  /** Sends a command; resolves once the engine accepts it. */
  send(command: Command): Promise<void> {
    const socket = this.#socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('not connected to the engine'));
    }
    const id = this.#nextId++;
    const message: ClientMessage = { id, command };
    socket.send(JSON.stringify(message));
    return new Promise((resolve, reject) => this.#pending.set(id, { resolve, reject }));
  }

  /** Like {@link send}, but shows failures in the UI instead of throwing. */
  run(command: Command): void {
    this.send(command).catch((e: Error) => (this.error = e.message));
  }

  close() {
    this.#closed = true;
    this.#socket?.close();
  }

  async #connect() {
    this.status = 'connecting';
    let socket: WebSocket;
    try {
      socket = new WebSocket(await engineUrl());
    } catch (e) {
      this.error = String(e);
      this.#retry();
      return;
    }
    this.#socket = socket;

    socket.onopen = () => {
      this.status = 'open';
      this.error = null;
      this.#backoff = 250;
    };
    socket.onmessage = (e) => this.#handle(JSON.parse(e.data) as ServerMessage);
    socket.onclose = () => {
      this.status = 'closed';
      for (const { reject } of this.#pending.values()) reject(new Error('connection closed'));
      this.#pending.clear();
      this.#retry();
    };
  }

  #retry() {
    if (this.#closed) return;
    setTimeout(() => void this.#connect(), this.#backoff);
    this.#backoff = Math.min(this.#backoff * 2, MAX_BACKOFF_MS);
  }

  #handle(message: ServerMessage) {
    switch (message.type) {
      case 'hello':
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
      case 'event':
        if (message.event.type === 'state') {
          this.player = message.event.state;
          this.receivedAt = performance.now();
        } else {
          this.queue = message.event.items;
        }
        break;
    }
  }
}

/**
 * Playback position extrapolated between engine snapshots, so the UI shows
 * progress without the engine streaming updates. It ticks once per second of
 * playback (enough for time labels); progress bars animate in between on
 * their own (see Slider's `running`), so nothing runs every frame.
 */
export class PlaybackPosition {
  #now = $state(performance.now());
  private engine: EngineClient;

  constructor(engine: EngineClient) {
    this.engine = engine;
  }

  get ms(): number {
    const player = this.engine.player;
    if (!player) return 0;
    if (player.status !== 'playing') return player.position_ms;
    const position = player.position_ms + Math.max(0, this.#now - this.engine.receivedAt);
    return Math.min(position, this.engine.current?.track.duration_ms ?? Infinity);
  }

  /** Call from an `$effect`; ticks on each whole second of playback while playing. */
  track(): (() => void) | undefined {
    if (this.engine.player?.status !== 'playing') return;
    let timer: ReturnType<typeof setTimeout>;
    const tick = () => {
      this.#now = performance.now();
      timer = setTimeout(tick, 1000 - (this.ms % 1000));
    };
    tick();
    return () => clearTimeout(timer);
  }
}
