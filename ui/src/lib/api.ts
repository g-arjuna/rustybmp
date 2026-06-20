/**
 * rustybmp HTTP API client (RV4-3)
 * Proxied via vite dev server → localhost:7878
 */

const BASE = '';  // relative, proxied by vite

export interface RouteRow {
  occurred_at:  string;
  speaker_addr: string;
  peer_addr:    string;
  peer_as:      number;
  rib_type:     string;
  action:       string;
  prefix:       string;
  afi:          string;
  origin:       string | null;
  as_path:      string | null;
  as_path_len:  number | null;
  next_hop:     string | null;
  local_pref:   number | null;
  med:          number | null;
  communities:  string | null;
}

export interface PeerSummary {
  peer_addr:    string;
  peer_as:      number;
  rib_type:     string;
  state:        string;
  prefix_count: number;
  hold_time:    number | null;
}

export interface SpeakerSummary {
  addr:        string;
  sys_name:    string | null;
  sys_descr:   string | null;
  peer_count:  number;
}

export interface TopologyGraph {
  nodes: { id: string; name: string | null; protocol: string | null }[];
  links: { source: string; target: string; igp_metric: number | null }[];
}

export interface Alert {
  kind:        string;
  prefix:      string;
  severity:    string;
  description: string;
  ts:          string;
}

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(BASE + path, window.location.href);
  if (params) {
    for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  }
  const res = await fetch(url.toString());
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json() as Promise<T>;
}

export const api = {
  health:    () => get<{ status: string }>('/health'),
  peers:     () => get<{ peers: unknown[] }>('/api/peers'),
  speakers:  () => get<{ speakers: unknown[] }>('/api/speakers'),
  routes:    (params?: Record<string, string>) => get<{ routes: RouteRow[] }>('/api/routes', params),
  churn:     () => get<{ prefixes: [string, number][] }>('/api/analytics/churn'),
  origins:   () => get<{ origins: [number, number][] }>('/api/analytics/origins'),
  bgplsGraph: (protocol?: string) =>
    get<TopologyGraph>('/api/bgpls/graph', protocol ? { protocol } : undefined),
  rpkiStats: () => get<Record<string, number>>('/api/rpki/stats'),

  // Prefix Explorer (RV5-2)
  prefixTimeline: (prefix: string, days = 7) =>
    get<{ prefix: string; timeline: { bucket: string; action: string; count: number }[] }>(
      `/api/routes/prefix/${encodeURIComponent(prefix)}/timeline`, { days: String(days) }),
  prefixPeers: (prefix: string) =>
    get<{ prefix: string; peers: unknown[]; count: number }>(
      `/api/routes/prefix/${encodeURIComponent(prefix)}/peers`),
  prefixConvergence: (prefix: string, limit = 50) =>
    get<{ prefix: string; events: unknown[] }>(
      `/api/routes/prefix/${encodeURIComponent(prefix)}/convergence`, { limit: String(limit) }),

  // RPKI analysis (RV5-4)
  rpkiAnalysis: () => get<{ breakdown: unknown[]; per_peer: unknown[] }>('/api/rpki/analysis'),

  // Policy delta (RV5-5)
  policyDelta: (peer: string) =>
    get<{ peer_addr: string; by_rib_type: unknown[] }>('/api/policy', { peer }),

  // Peer session timeline (RV5-6)
  peerTimeline: (addr: string, days = 7) =>
    get<{ peer_addr: string; timeline: unknown[] }>(
      `/api/peers/${encodeURIComponent(addr)}/timeline`, { days: String(days) }),

  // ML anomalies (RV5-9)
  mlAnomalies: (limit = 100, kind?: string) =>
    get<{ anomalies: unknown[]; count: number }>(
      '/api/ml/anomalies', { limit: String(limit), ...(kind ? { kind } : {}) }),
};

/** Open the SSE /api/events stream and call onEvent for each event. */
export function openEventStream(
  onEvent: (type: string, data: unknown) => void,
): EventSource {
  const es = new EventSource('/api/events');
  es.onmessage = (e) => {
    try {
      const parsed = JSON.parse(e.data as string) as { type: string; data: unknown };
      onEvent(parsed.type, parsed.data);
    } catch {
      onEvent('raw', e.data);
    }
  };
  return es;
}
