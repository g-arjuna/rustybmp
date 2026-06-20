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
  peers:     () => get<PeerSummary[]>('/api/peers'),
  speakers:  () => get<SpeakerSummary[]>('/api/speakers'),
  routes:    (params?: Record<string, string>) => get<RouteRow[]>('/api/routes', params),
  churn:     () => get<[string, number][]>('/api/analytics/churn'),
  origins:   () => get<[number, number][]>('/api/analytics/origins'),
  bgplsGraph: (protocol?: string) =>
    get<TopologyGraph>('/api/bgpls/graph', protocol ? { protocol } : undefined),
  rpkiStats: () => get<Record<string, number>>('/api/rpki/stats'),
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
