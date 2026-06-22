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

  // ── RV6 new endpoints ──────────────────────────────────────────────────────

  // AS Path graph (RV6-5)
  asPathGraph: (asn?: number, peer?: string, limit = 200) =>
    get<{ nodes: { id: string; label: string }[]; links: { source: string; target: string; value: number }[] }>(
      '/api/aspath/graph', {
        limit: String(limit),
        ...(asn  ? { asn: String(asn) }   : {}),
        ...(peer ? { peer }               : {}),
      }),

  // SR Policy (RV6-5)
  srpolicyList: (limit = 200) =>
    get<{ policies: unknown[]; count: number }>('/api/srpolicy', { limit: String(limit) }),
  srpolicyByPeer: (peer: string, limit = 200) =>
    get<{ peer: string; policies: unknown[]; count: number }>(
      `/api/srpolicy/${encodeURIComponent(peer)}`, { limit: String(limit) }),

  // BMP stats history (RV6-5)
  bmpStatsHistory: (peer?: string, limit = 200) =>
    get<{ stats: unknown[]; count: number }>(
      '/api/bmpstats/history', { limit: String(limit), ...(peer ? { peer } : {}) }),

  // Peer capabilities (RV6-5)
  peerCapabilities: (addr: string) =>
    get<{ peer_addr: string; peer_as: number; capabilities: { code: number; name: string }[]; hold_time: number; add_path: boolean; four_byte_asn: boolean; llgr: boolean }>(
      `/api/peers/${encodeURIComponent(addr)}/capabilities`),

  // RPKI coverage (RV6-5)
  rpkiCoverage: () =>
    get<{ total_prefixes: number; covered: number; not_covered: number; valid: number; invalid: number; coverage_pct: number }>(
      '/api/rpki/coverage'),

  // BGP-LS path (RV6-5)
  bgplsPath: (from: string, to: string) =>
    get<{ from: string; to: string; path: string[]; found: boolean }>(
      '/api/bgpls/path', { from, to }),

  // ML model status (RV6-5)
  mlModelStatus: () =>
    get<{ models: { model: string; path: string; ready: boolean; status: string }[] }>(
      '/api/ml/model/status'),

  // Path Status matrix (RV7-P3)
  pathStatusMatrix: (params?: Record<string, string>) =>
    get<{ rows: unknown[]; count: number }>('/api/path-status/matrix', params),
  pathStatusHistory: (prefix: string, peer?: string, limit = 200) =>
    get<{ rows: unknown[] }>(
      '/api/path-status/history',
      { prefix, limit: String(limit), ...(peer ? { peer } : {}) }),

  // Max-prefix capacity (RV7-B4)
  maxPrefixCapacity: () =>
    get<{ rows: unknown[]; count: number }>('/api/capacity/max-prefix'),

  // Policy configs (RV7-B4)
  policyConfigs: (peer?: string) =>
    get<{ rows: unknown[]; count: number }>(
      peer ? `/api/policy/configs/${encodeURIComponent(peer)}` : '/api/policy/configs'),

  // Filter management (RV6-1)
  filterStats: () =>
    get<{ filter_file: string; filter_count: number; counters: Record<string, string> }>(
      '/api/filters/stats'),
  filterTest: (body: { prefix: string; peer_as: number; as_path?: string; rpki?: string; communities?: string[] }) =>
    fetch('/api/filters/test', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(r => r.json()),
  filterReload: () =>
    fetch('/api/filters/reload', { method: 'POST' }).then(r => r.json()),

  // ── RV8 new endpoints ──────────────────────────────────────────────────────

  // Adaptive homepage: per-speaker aggregated summary (RV8-UX3)
  speakersSummary: () =>
    get<{
      speakers: {
        addr: string; hostname: string; vendor: string; bmp_state: string;
        peers_up: number; peers_down: number; route_count: number; connected_at: string;
      }[];
      count: number; total_peers_up: number; total_routes: number;
      has_speakers: boolean; has_active_sessions: boolean;
    }>('/api/speakers/summary'),

  // Resource governor status (RV8-GOV2)
  governance: () =>
    get<{
      profile: string; memory_budget_mb: number; rate_budget_eps: number;
      memory_pressure_active: boolean; write_pressure_active: boolean;
      rate_shedding_active: boolean; memory_shrink_count: number;
      write_batch_expand_count: number; rate_shed_count: number;
    }>('/api/governance'),

  // External prefix visibility (RV8-EXT5)
  prefixVisibility: (prefix: string) =>
    get<{ prefix: string; internal: unknown; external: unknown; discrepancies: string[] }>(
      '/api/external/prefix-visibility', { prefix }),

  // ── RV9 new endpoints ──────────────────────────────────────────────────────

  // NL query (RV9-UX1)
  nlQuery: (query: string) =>
    fetch('/api/nl-query', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ query }),
    }).then(r => r.json() as Promise<{ sql: string; rows: Record<string, unknown>[]; error?: string }>),

  // Output adapters (RV9-UX2 / OUT6)
  adaptersList: () =>
    get<{ adapters: { name: string; kind: string; enabled: boolean; healthy: boolean; last_push_at: string | null; event_count: number; error: string | null }[] }>(
      '/api/adapters'),
  adapterTest: (name: string) =>
    fetch(`/api/adapters/${encodeURIComponent(name)}/test`, { method: 'POST' })
      .then(r => r.json() as Promise<{ ok: boolean; message: string }>),

  // Communities explorer (RV9-UX3)
  communities: () =>
    get<{ communities: { community: string; route_count: number; pre_policy: number; post_policy: number; first_seen: string | null; last_changed: string | null }[] }>(
      '/api/communities'),
  communitySemantics: () =>
    get<{ semantics: { community: string; meaning: string; confidence: number; pattern: string | null }[] }>(
      '/api/communities/semantics'),

  // FlowSpec rules (RV9-UX5)
  flowspecRules: (speaker?: string) =>
    get<{ rules: unknown[] }>(
      '/api/flowspec/rules', speaker ? { speaker } : undefined),

  // VRF explorer (RV9-UX6)
  vrfList: () =>
    get<{ vrfs: { rd: string; vrf_name: string | null; route_count: number; peer_count: number; afi: string }[] }>(
      '/api/vrf/list'),
  vrfRoutes: (rd: string, limit = 500) =>
    get<{ routes: unknown[] }>(
      `/api/vrf/${encodeURIComponent(rd)}/routes`, { limit: String(limit) }),
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
