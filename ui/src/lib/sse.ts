/**
 * RV6-3: SSE client with RAF-batched updates.
 *
 * Wraps the native EventSource with:
 *  - Automatic reconnection (exponential backoff, max 30 s)
 *  - requestAnimationFrame batching so bursts of events don't
 *    thrash reactive stores / DOM at sub-16 ms intervals
 *  - Token-header injection (uses ?token= query param for EventSource)
 */

export interface SseEvent {
  type:    string;
  payload: unknown;
}

export type SseCallback = (events: SseEvent[]) => void;

const BACKOFF_BASE  = 500;   // ms
const BACKOFF_MAX   = 30_000; // ms

export class SseClient {
  private url:        string;
  private token:      string | null;
  private onBatch:    SseCallback;
  private es:         EventSource | null = null;
  private pending:    SseEvent[]         = [];
  private rafHandle:  number | null      = null;
  private attempt:    number             = 0;
  private closed:     boolean            = false;
  private reconnTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(path: string, onBatch: SseCallback, token?: string) {
    this.url     = path;
    this.token   = token ?? null;
    this.onBatch = onBatch;
    this.connect();
  }

  private connect() {
    if (this.closed) return;
    const url = this.token ? `${this.url}?token=${encodeURIComponent(this.token)}` : this.url;
    this.es = new EventSource(url);

    this.es.onmessage = (ev) => {
      try {
        const raw = JSON.parse(ev.data);
        this.pending.push({ type: raw.type ?? 'message', payload: raw });
      } catch {
        this.pending.push({ type: 'message', payload: ev.data });
      }
      this.scheduleFlush();
    };

    this.es.addEventListener('route_event', (ev) => {
      try { this.pending.push({ type: 'route_event', payload: JSON.parse((ev as MessageEvent).data) }); }
      catch { /* ignore */ }
      this.scheduleFlush();
    });

    this.es.onerror = () => {
      this.es?.close();
      this.es = null;
      this.scheduleReconnect();
    };

    this.es.onopen = () => { this.attempt = 0; };
  }

  private scheduleFlush() {
    if (this.rafHandle !== null) return;
    this.rafHandle = requestAnimationFrame(() => {
      this.rafHandle = null;
      if (this.pending.length === 0) return;
      const batch = this.pending.splice(0);
      try { this.onBatch(batch); } catch { /* ignore callback errors */ }
    });
  }

  private scheduleReconnect() {
    if (this.closed) return;
    const delay = Math.min(BACKOFF_BASE * 2 ** this.attempt, BACKOFF_MAX);
    this.attempt++;
    this.reconnTimer = setTimeout(() => { this.connect(); }, delay);
  }

  /** Permanently close this SSE connection. */
  close() {
    this.closed = true;
    this.es?.close();
    this.es = null;
    if (this.rafHandle !== null) { cancelAnimationFrame(this.rafHandle); this.rafHandle = null; }
    if (this.reconnTimer !== null) { clearTimeout(this.reconnTimer); this.reconnTimer = null; }
  }
}

/** Svelte-lifecycle helper: start SSE on mount, close on destroy. */
export function openEventStream(
  path: string,
  onBatch: SseCallback,
  token?: string,
): SseClient {
  return new SseClient(path, onBatch, token);
}
