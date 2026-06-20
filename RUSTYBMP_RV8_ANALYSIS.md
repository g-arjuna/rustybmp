# RustyBMP — Sprint RV8 Analysis
## Swagger · MCP Server · Output Adapters · Enrichers · ML Depth · External Integrations

> **Context**: Written while RV7 is being coded.
> **Source**: Deep read of bonsai integrations:
> — `src/output/`: elastic.rs, splunk_hec.rs, servicenow_em.rs, prometheus.rs, traits.rs
> — `src/integrations/`: servicenow_aiops.rs, change_management.rs
> — `src/enrichment/`: servicenow.rs (CMDB input), netbox.rs (dual REST/MCP transport)
> — `src/mcp_server.rs`: JSON-RPC 2.0 tools, rule catalogue with recurrence indicators
> — `src/mcp_client.rs`: shared MCP transport (REST|MCP dual mode)
> — `src/http_server/schema.rs`: full OpenAPI 3.0.3 with 17 tag groups
> — `src/http_server/nl_query.rs`: NL→Cypher with 500K/day token budget
> — `src/http_server/ml_jobs.rs`: Parquet catalog, GNN results, embeddings, ML SSE bus

---

## Part 1 — Bonsai Integration Inventory: What Exists

### Output adapters (bonsai → external systems)

| Adapter | Protocol | Auth | What gets pushed |
|---------|----------|------|-----------------|
| **Elasticsearch** | Bulk `_bulk` ndjson | Basic or API key | DetectionEvents as ECS documents |
| **Splunk HEC** | `/services/collector/event` | Token from vault | DetectionEvents as structured JSON |
| **ServiceNow EM** | `/api/now/table/em_event` | Basic from vault | DetectionEvents as `em_event` records |
| **Prometheus** | Remote-write protobuf | Bearer/Basic | Raw telemetry counters (collector-side) |
| **SNMP adapter** | SNMP traps | Community string | StateChangeEvents as traps |
| **Syslog adapter** | RFC 5424 UDP/TCP | None | DetectionEvents as syslog messages |

All adapters share the `OutputAdapter` trait:
- Runs as independent background tokio task (failure-isolated)
- Polls DuckDB/graph on a configurable flush interval
- Cursor-based polling (persisted to disk, survives restarts without re-pushing)
- In-memory dedup window by `(device, rule)` pair
- Every push cycle writes to audit log via `OutputAdapterAuditLog`
- Credential resolved from vault (never in config)
- Environment-scoped (can limit to specific network segments)

### Input enrichers (external → bonsai graph)

| Enricher | Source | What it writes |
|----------|--------|---------------|
| **ServiceNow CMDB** | cmdb_ci_business_service, cmdb_rel_ci, cmdb_ci_server | Application nodes, CI properties (OS, RAM, serial), site/location, CMDB_PARENT_OF edges |
| **NetBox** | /api/dcim/devices, /api/ipam/vlans, /api/ipam/prefixes | Device properties (model, serial, role), VLAN + Prefix nodes, HostEndpoint nodes |

NetBox enricher has **dual transport**: standard REST or MCP proxy (selected via `config.extra.transport = "mcp"`). The `McpClient` handles the JSON-RPC wrapping transparently.

### MCP server (bonsai as MCP provider)

Bonsai exposes `POST /mcp` as a JSON-RPC 2.0 endpoint serving five tools:
- `get_incident` — full incident by root detection ID (includes blast radius + rule documentation)
- `query_devices` — devices matching hostname/address substring filter
- `get_device_blast_radius` — reachable impact set within N hops
- `list_active_detections` — recent detections with optional severity/rule filter
- `query_graph` — read-only Cypher passthrough against the graph DB

Additionally:
- `GET /api/openapi.json` — OpenAPI 3.0.3 spec
- `GET /api/swagger` — Swagger UI (inline HTML, SwaggerUIBundle from unpkg CDN)
- `GET /api/resolve?q=X` — fuzzy device/detection name resolver for AI tool disambiguation
- `GET /api/nl-query` — plain English → Cypher with 500K token/day budget

### ML pipeline APIs (bonsai's ML infra)

Bonsai separates ML concerns across two boundaries:
1. **Python sidecar** does all training/inference (PyATS, PyG, scikit-learn)
2. **Rust core** hosts the API endpoints that:
   - Accept inference results from Python (POST /api/gnn/inference-results)
   - Store per-event embeddings (POST /api/events/embeddings)
   - Manage Parquet export catalog (GET/POST /api/ml/exports with schema hash + quality metrics)
   - Broadcast ML events via dedicated SSE bus (`/api/ml/events/stream`)
   - Manage job runs and schedules (GET/POST/PATCH /api/ml/jobs)

This pattern is already partially used in rustybmp: the ML anomalies table is written by Python and read by Rust. The bonsai pattern extends this to: Python publishes via SSE bus → UI subscribes and shows live model training progress.

---

## Part 2 — What's Directly Relevant for rustybmp

### 2.1 Relevance matrix

| Bonsai feature | rustybmp relevance | Reuse method |
|---------------|-------------------|--------------|
| Elasticsearch adapter | ✅ HIGH — BGP events to SIEM | Copy OutputAdapter trait + elastic.rs with BGP event schema |
| Splunk HEC adapter | ✅ HIGH — BGP events to Splunk | Copy OutputAdapter trait + splunk_hec.rs |
| ServiceNow EM adapter | ✅ HIGH — BGP alerts to ITSM | Copy servicenow_em.rs; same em_event API |
| Prometheus adapter | ✅ DONE — already in RV4 via metrics crate | Already implemented |
| ServiceNow CMDB enricher | ✅ MEDIUM — router CI context for speakers | Adapt servicenow.rs; pull router model/site/assignment_group |
| NetBox enricher | ✅ MEDIUM — IP ownership + site for speakers | Adapt netbox.rs; pull speaker site/region context |
| MCP server | ✅ HIGH — AI agents need BGP visibility | New tool set; same JSON-RPC 2.0 pattern |
| MCP client (dual transport) | ✅ MEDIUM — enrichers can go via MCP | Copy mcp_client.rs verbatim |
| Swagger UI + OpenAPI spec | ✅ HIGH — operator tooling + LLM tool descriptions | Build from scratch; bonsai schema.rs is the template |
| NL→Cypher | ✅ HIGH (adapted) — NL→DuckDB SQL | Adapt nl_query.rs; DuckDB SQL instead of Cypher |
| SNMP adapter | ❌ LOW — routers already send SNMP separately | Skip |
| Syslog adapter | ❌ LOW — BGP events are structured, not syslog | Skip |
| OutputAdapter trait | ✅ COPY — universal pattern for all push adapters | Copy traits.rs; change OutputTopic from graph to DuckDB |
| SNMP adapter | ❌ LOW | Skip |
| GNN inference results | ✅ MEDIUM — STGNN writes back to Rust | Adapt for BGP STGNN results |
| ML SSE bus | ✅ MEDIUM — show training progress in UI | Copy ml_event_bus.rs |
| Parquet export catalog | ✅ DONE — already tracking in bmppy | Already implemented differently |
| Dedup window logic | ✅ COPY — in-memory HashMap (device, rule) → last_pushed_ns | Identical pattern needed |
| Cursor persistence | ✅ COPY — `.cursor` file per adapter | Identical pattern needed |

---

## Part 3 — The OutputAdapter Trait for rustybmp

The central design insight from bonsai: **a single trait handles all push adapters**. Copy this pattern exactly.

### 3.1 Adapted trait for rustybmp

```rust
// crates/rbmp-server/src/output/traits.rs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTopic {
    /// Route events (announce/withdraw, anomaly-scored)
    RouteEvents,
    /// Peer session events (up/down/flap)
    PeerEvents,
    /// ML anomaly detections
    AnomalyDetections,
    /// Path status events (from Path Status TLV)
    PathStatusEvents,
    /// RPKI validation changes
    RpkiEvents,
    /// All events combined (for full-fidelity outputs like Kafka)
    AllEvents,
}

#[async_trait::async_trait]
pub trait OutputAdapter: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn topics(&self) -> &[OutputTopic];
    /// Speakers to include (empty = all)
    fn speaker_scope(&self) -> &[String];
    async fn run(
        &self,
        creds: Arc<CredentialVault>,
        audit: OutputAdapterAuditLog,
        shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<()>;
    async fn test_connection(&self, creds: Arc<CredentialVault>) -> anyhow::Result<()>;
}
```

### 3.2 What each adapter pushes — BGP event schema

All push adapters query DuckDB for recent events using this canonical query:

```sql
-- Route anomaly events (for SIEM outputs)
SELECT r.occurred_at, r.speaker_addr, r.peer_addr, r.peer_as,
       r.prefix, r.afi, r.action, r.as_path, r.rpki_validity,
       r.communities, r.local_pref, r.med,
       a.kind AS anomaly_kind, a.score, a.severity, a.description
FROM route_events r
LEFT JOIN ml_anomalies a ON a.prefix = r.prefix
    AND a.peer_addr = r.peer_addr
    AND a.detected_at BETWEEN r.occurred_at - INTERVAL '5 minutes'
                          AND r.occurred_at + INTERVAL '5 minutes'
WHERE r.occurred_at > TIMESTAMPTZ $cursor
  AND ($speaker_scope = '' OR r.speaker_addr = ANY($speaker_scope))
ORDER BY r.occurred_at ASC
LIMIT 500
```

---

## Part 4 — Elasticsearch Adapter

### Event schema: BGP route events as ECS documents

Bonsai maps to ECS. rustybmp does the same for BGP events:

```rust
fn route_event_to_ecs(row: &RouteEventRow) -> serde_json::Value {
    serde_json::json!({
        // ECS standard fields
        "@timestamp":       row.occurred_at,
        "event.kind":       if row.anomaly_kind.is_some() { "alert" } else { "event" },
        "event.category":   ["network"],
        "event.type":       [if row.action == "announce" { "creation" } else { "deletion" }],
        "event.severity":   match row.anomaly_severity.as_deref() {
                                Some("critical") => 1,
                                Some("warn") => 3,
                                _ => 5,
                            },
        "event.module":     "rustybmp",
        "event.dataset":    "rustybmp.route_events",
        "host.ip":          [row.speaker_addr],
        "source.as.number": row.peer_as,
        "source.ip":        row.peer_addr,
        "network.protocol": "bgp",
        // BGP-specific fields
        "rustybmp.prefix":       row.prefix,
        "rustybmp.afi":          row.afi,
        "rustybmp.action":       row.action,
        "rustybmp.as_path":      row.as_path,
        "rustybmp.rpki":         row.rpki_validity,
        "rustybmp.communities":  row.communities,
        "rustybmp.local_pref":   row.local_pref,
        "rustybmp.speaker_addr": row.speaker_addr,
        "rustybmp.peer_as":      row.peer_as,
        // Anomaly enrichment (when present)
        "rustybmp.anomaly.kind":        row.anomaly_kind,
        "rustybmp.anomaly.score":       row.anomaly_score,
        "rustybmp.anomaly.description": row.anomaly_description,
        // SIEM-friendly tags
        "tags": ["rustybmp", "bgp", row.afi.as_str(), row.action.as_str()],
    })
}
```

**Config**:
```toml
[[output_adapters]]
name = "elastic-bgp"
type = "elasticsearch"
endpoint_url = "https://my-elastic.example.com:9200"
credential_alias = "elastic-api-key"
flush_interval_secs = 30
dedup_window_secs = 300  # suppress same (peer, anomaly_kind) within 5 min

[output_adapters.extra]
index = "rustybmp-bgp-events"
auth_type = "api_key"  # or "basic"
topics = ["anomaly_detections", "peer_events"]  # filter which topics to push
```

---

## Part 5 — Splunk HEC Adapter

Same push pattern as Elastic; different wire format.

```rust
fn route_event_to_hec(row: &RouteEventRow, sourcetype: &str, index: &str) -> serde_json::Value {
    serde_json::json!({
        "time":       chrono::DateTime::parse_from_rfc3339(&row.occurred_at)
                          .map(|dt| dt.timestamp_millis() as f64 / 1000.0)
                          .unwrap_or(0.0),
        "host":       row.speaker_addr,
        "source":     format!("rustybmp:{}", row.speaker_addr),
        "sourcetype": sourcetype,   // default: "rustybmp:route_event"
        "index":      index,
        "event": {
            "prefix":       row.prefix,
            "afi":          row.afi,
            "action":       row.action,
            "peer_as":      row.peer_as,
            "peer_addr":    row.peer_addr,
            "speaker_addr": row.speaker_addr,
            "as_path":      row.as_path,
            "rpki":         row.rpki_validity,
            "communities":  row.communities,
            "anomaly_kind": row.anomaly_kind,
            "anomaly_score": row.anomaly_score,
            "severity":     row.anomaly_severity,
        }
    })
}
```

**Config**:
```toml
[[output_adapters]]
name = "splunk-bgp"
type = "splunk_hec"
endpoint_url = "https://splunk.example.com:8088"
credential_alias = "splunk-hec-token"   # password field = HEC token
flush_interval_secs = 60

[output_adapters.extra]
sourcetype = "rustybmp:bgp"
index = "network_bgp"
```

---

## Part 6 — ServiceNow EM Adapter

BGP anomalies as ServiceNow Event Management events. The ServiceNow `em_event` table is the same endpoint bonsai uses — copy the HTTP call pattern exactly:

```rust
async fn push_event_to_snow(
    instance_url: &str,
    username: &str,
    password: &str,
    row: &AnomalyRow,
) -> anyhow::Result<()> {
    let severity_map = |s: &str| match s {
        "critical" => 1u8,
        "high"     => 2u8,
        "warn"     => 3u8,
        "info"     => 5u8,
        _          => 5u8,
    };

    let payload = serde_json::json!({
        "records": [{
            "source":              "RustyBMP",
            "event_class":        "BGP",
            "resource":            row.prefix,
            "node":                row.speaker_addr,
            "severity":            severity_map(&row.severity),
            "description":         row.description,
            "additional_info":    serde_json::json!({
                "prefix":          row.prefix,
                "peer_addr":       row.peer_addr,
                "peer_as":         row.peer_as,
                "anomaly_kind":    row.anomaly_kind,
                "anomaly_score":   row.anomaly_score,
                "rpki":            row.rpki_validity,
                "as_path":         row.as_path,
            }).to_string(),
            "message_key":         format!("{}:{}:{}", row.speaker_addr, row.peer_addr, row.anomaly_kind),
            "time_of_event":       row.occurred_at,
        }]
    });

    let client = reqwest::Client::new();
    let url = format!("{instance_url}/api/now/table/em_event");
    let resp = client.post(&url)
        .basic_auth(username, Some(password))
        .json(&payload)
        .send().await?;

    if !resp.status().is_success() {
        anyhow::bail!("ServiceNow EM push failed: {}", resp.status());
    }
    Ok(())
}
```

**Bidirectional ServiceNow** (for RV8 stretch goal): ServiceNow AIOps sync. When a BGP anomaly is correlated to a ServiceNow incident, rustybmp can:
- Create a new INC with the BGP anomaly details
- Update the INC when the anomaly resolves (add work note "BGP peer recovered")
- Pull back the INC state to mark detection as acknowledged

---

## Part 7 — Webhook Adapter (New — not in bonsai)

Webhooks are the most operator-requested integration pattern in 2026. A generic webhook adapter covers Slack, Teams, Discord, PagerDuty, OpsGenie, and any custom endpoint:

```rust
pub struct WebhookAdapter {
    config:   OutputAdapterConfig,
    template: WebhookTemplate,
}

#[derive(Deserialize)]
struct WebhookTemplate {
    method:   String,         // GET | POST | PUT
    headers:  HashMap<String, String>,
    body_template: String,    // Handlebars template
    // Pre-built profiles
    profile:  Option<String>, // "slack" | "pagerduty" | "opsgenie" | "teams" | "custom"
}
```

Pre-built profiles:
```toml
# Slack
[[output_adapters]]
name = "slack-bgp-alerts"
type = "webhook"
endpoint_url = "https://hooks.slack.com/services/T.../B.../..."

[output_adapters.extra]
profile = "slack"
channel = "#bgp-alerts"
min_severity = "warn"

# PagerDuty
[[output_adapters]]
name = "pagerduty-bgp"
type = "webhook"
endpoint_url = "https://events.pagerduty.com/v2/enqueue"

[output_adapters.extra]
profile = "pagerduty"
routing_key_alias = "pd-bgp-key"  # from vault
deduplicate_by = ["prefix", "peer_as", "anomaly_kind"]
```

Slack profile body (Blocks format):
```json
{
  "channel": "{{channel}}",
  "blocks": [
    {"type": "header", "text": {"type": "plain_text", "text": "{{severity_emoji}} BGP Anomaly: {{anomaly_kind}}"}},
    {"type": "section", "fields": [
      {"type": "mrkdwn", "text": "*Prefix:* `{{prefix}}`"},
      {"type": "mrkdwn", "text": "*Peer:* AS{{peer_as}} ({{peer_addr}})"},
      {"type": "mrkdwn", "text": "*Speaker:* {{speaker_addr}}"},
      {"type": "mrkdwn", "text": "*RPKI:* {{rpki}}"}
    ]},
    {"type": "section", "text": {"type": "mrkdwn", "text": "{{description}}"}}
  ]
}
```

---

## Part 8 — NetBox Enricher (Adapted from Bonsai)

Bonsai's `netbox.rs` pulls device/site/VLAN/prefix from NetBox, supports both REST and MCP transport. For rustybmp, the purpose is different: enrich BMP speaker records with site context, ownership, and IP block information.

**What to pull from NetBox for rustybmp**:

| NetBox object | What we get | Where it's stored |
|--------------|-------------|------------------|
| `dcim/devices` (by mgmt IP) | Site, rack, role, model, serial, owner | `speaker_registry` DuckDB table |
| `ipam/ip-addresses` | Prefix block, VRF, description | Enriches route_events |
| `ipam/prefixes` | Owner AS, site, utilization | Prefix ownership info |
| `ipam/vlans` | VLAN name, site | EVPN VLAN context |

```rust
// crates/rbmp-enrichment/src/netbox.rs
// Same dual REST/MCP transport as bonsai — copy the McpClient pattern

pub struct NetboxEnricher {
    config:    EnricherConfig,
    transport: EnricherTransport,  // REST | MCP (from bonsai mcp_client.rs)
}

impl NetboxEnricher {
    async fn fetch_device_by_ip(&self, ip: &str, cred: &ResolvedCredential) -> anyhow::Result<Option<NetboxDevice>> {
        match &self.transport {
            EnricherTransport::Rest => {
                // Direct REST call
                let url = format!("{}/api/dcim/devices/?primary_ip={ip}", self.config.endpoint_url);
                ...
            }
            EnricherTransport::Mcp { server_url } => {
                // Proxy through MCP server
                self.mcp_client.call("netbox:devices_list", json!({
                    "primary_ip": ip, "format": "json"
                })).await
            }
        }
    }
}
```

**Config**:
```toml
[enrichers.netbox]
enabled = true
endpoint_url = "https://netbox.example.com"
credential_alias = "netbox-token"
poll_interval_secs = 3600  # Re-enrich every hour

[enrichers.netbox.extra]
transport = "rest"   # or "mcp" with mcp_server_url
```

---

## Part 9 — ServiceNow CMDB Enricher (Adapted from Bonsai)

Bonsai's `servicenow.rs` pulls Application nodes and CI properties. For rustybmp, pull router CI information to enrich BMP speaker records:

**What to pull**:
- Router model, serial number from `cmdb_ci_network_gear`
- Site/location from `cmn_location`
- Assignment group (who owns this router?) from `cmdb_ci_router`
- Associated change requests (for policy change correlation)
- Open incidents (is this router already under investigation?)

This enables the policy change correlation engine: when a BGP policy change is detected via BMP pre/post diff at 09:44, cross-reference ServiceNow change requests opened for that router in the same time window.

```toml
[enrichers.servicenow_cmdb]
enabled = true
instance_url = "https://mycompany.service-now.com"
credential_alias = "snow-api-ro"

[enrichers.servicenow_cmdb.extra]
pull_change_requests = true   # Enable change-to-policy-change correlation
ci_table = "cmdb_ci_router"   # Network router CI table
```

---

## Part 10 — MCP Server for rustybmp

Bonsai's MCP server exposes read-only network state to AI agents. rustybmp needs an equivalent that exposes BGP/BMP state. The MCP server is **the highest-value RV8 addition** because it makes rustybmp natively consumable by AI agents (Claude, Cursor, GPT-4, etc.) without any custom integration.

### 10.1 Architecture (identical to bonsai)

```
POST /mcp   — JSON-RPC 2.0
GET  /mcp/sse — SSE stream of tool results (optional, for streaming responses)
```

### 10.2 Tool definitions

```rust
// crates/rbmp-server/src/mcp_server.rs

pub static BGP_TOOLS: &[McpTool] = &[
    McpTool {
        name: "get_prefix_status",
        description: "Get the current BGP routing status for a specific IP prefix. Returns path states per peer (best/ECMP/backup/filtered/invalid), RPKI validity, AS path, and convergence time.",
        parameters: &[
            Param { name: "prefix", description: "IP prefix in CIDR notation, e.g. '203.0.113.0/24'", required: true },
            Param { name: "afi",    description: "'ipv4' or 'ipv6' (default: ipv4)", required: false },
        ],
    },
    McpTool {
        name: "get_peer_health",
        description: "Get BGP peer session health including state, route counts, flap history, RPKI invalid rate, and max-prefix utilization.",
        parameters: &[
            Param { name: "peer_addr", description: "Peer IP address", required: true },
            Param { name: "speaker_addr", description: "BMP speaker address (optional, defaults to all speakers)", required: false },
        ],
    },
    McpTool {
        name: "query_route_events",
        description: "Query recent BGP route change events. Filter by prefix, peer AS, speaker, action (announce/withdraw), or RPKI state. Returns up to 100 events with timestamps.",
        parameters: &[
            Param { name: "prefix",      description: "IP prefix to filter by (optional)", required: false },
            Param { name: "peer_as",     description: "Peer AS number to filter by (optional)", required: false },
            Param { name: "speaker_addr",description: "BMP speaker IP (optional)", required: false },
            Param { name: "action",      description: "'announce' or 'withdraw' (optional)", required: false },
            Param { name: "rpki",        description: "'valid', 'invalid', or 'not-found' (optional)", required: false },
            Param { name: "since_hours", description: "Look back N hours (default: 1)", required: false },
            Param { name: "limit",       description: "Max results (default: 50, max: 200)", required: false },
        ],
    },
    McpTool {
        name: "get_anomalies",
        description: "Get recent BGP anomaly detections from the ML pipeline. Includes hijack, route leak, flap, origin change, and RPKI violation anomalies with confidence scores.",
        parameters: &[
            Param { name: "kind",        description: "Anomaly type filter: 'hijack', 'leak', 'flap', 'origin_change', 'churn' (optional)", required: false },
            Param { name: "severity",    description: "'info', 'warn', 'critical' (optional)", required: false },
            Param { name: "prefix",      description: "Filter by prefix (optional)", required: false },
            Param { name: "since_hours", description: "Look back N hours (default: 24)", required: false },
            Param { name: "limit",       description: "Max results (default: 20)", required: false },
        ],
    },
    McpTool {
        name: "get_rpki_status",
        description: "Get RPKI validation status for a prefix. Returns validity (valid/invalid/not-found), ROA details, max-length, and which peers advertise with each validity state.",
        parameters: &[
            Param { name: "prefix", description: "IP prefix in CIDR notation", required: true },
        ],
    },
    McpTool {
        name: "get_topology",
        description: "Get BGP-LS network topology including routers, links, IGP metrics, and SR adjacency SIDs. Returns nodes and links suitable for path computation.",
        parameters: &[
            Param { name: "protocol", description: "Filter by protocol: 'isis', 'ospf', 'direct' (optional, default: all)", required: false },
            Param { name: "max_nodes", description: "Maximum nodes to return (default: 500)", required: false },
        ],
    },
    McpTool {
        name: "compute_igp_path",
        description: "Compute the shortest IGP path between two routers using BGP-LS topology data and Dijkstra over IGP metrics.",
        parameters: &[
            Param { name: "src_router_id", description: "Source router ID (IP address)", required: true },
            Param { name: "dst_router_id", description: "Destination router ID (IP address)", required: true },
        ],
    },
    McpTool {
        name: "test_filter",
        description: "Test a Roto filter expression against a synthetic BGP route. Returns 'accept' or 'reject' with evaluation time.",
        parameters: &[
            Param { name: "prefix",      description: "Test prefix (e.g. '10.0.0.0/8')", required: true },
            Param { name: "rpki",        description: "'valid', 'invalid', or 'not-found'", required: false },
            Param { name: "peer_as",     description: "Test peer AS number", required: false },
            Param { name: "as_path",     description: "Space-separated AS path (e.g. '65001 64496')", required: false },
            Param { name: "communities", description: "Comma-separated communities (e.g. '65001:100,no-export')", required: false },
        ],
    },
    McpTool {
        name: "get_capacity_status",
        description: "Get max-prefix capacity status per peer per AFI/SAFI. Returns current utilization, configured limit, growth trend, and estimated days to limit (RFC 9972 type 30).",
        parameters: &[
            Param { name: "peer_addr",   description: "Filter by peer address (optional)", required: false },
            Param { name: "min_pct",     description: "Only return peers above this utilization % (optional)", required: false },
        ],
    },
    McpTool {
        name: "natural_language_query",
        description: "Convert a plain English question about BGP routing to a DuckDB SQL query and execute it. Examples: 'Show all RPKI invalid routes in the last 24 hours', 'Which peers have more than 10000 routes?', 'Show route changes for prefix 203.0.113.0/24 this week'.",
        parameters: &[
            Param { name: "question", description: "Plain English question about BGP routing data", required: true },
        ],
    },
    McpTool {
        name: "get_convergence_events",
        description: "Get recent BGP convergence events showing how long it took for routing to stabilize after a peer-down or mass-withdrawal event.",
        parameters: &[
            Param { name: "peer_addr",   description: "Filter by peer (optional)", required: false },
            Param { name: "since_hours", description: "Look back N hours (default: 24)", required: false },
        ],
    },
];
```

### 10.3 Implementation skeleton

```rust
// crates/rbmp-server/src/mcp_server.rs

pub async fn mcp_handler(
    State(state): State<AppState>,
    Json(req): Json<McpRequest>,
) -> Result<Json<McpResponse>, StatusCode> {
    match req.method.as_str() {
        "initialize" => Ok(Json(McpResponse::initialize(
            "rustybmp",
            env!("CARGO_PKG_VERSION"),
            BGP_TOOLS,
        ))),
        "tools/list" => Ok(Json(McpResponse::tools_list(BGP_TOOLS))),
        "tools/call" => {
            let tool_name = req.params.get("name").and_then(|v| v.as_str())
                .ok_or(StatusCode::BAD_REQUEST)?;
            let args = req.params.get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));

            let result = call_tool(&state, tool_name, args).await?;
            Ok(Json(McpResponse::tool_result(result)))
        }
        _ => Err(StatusCode::NOT_FOUND),
    }
}

async fn call_tool(state: &AppState, tool: &str, args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    match tool {
        "get_prefix_status"      => tools::get_prefix_status(state, args).await,
        "get_peer_health"        => tools::get_peer_health(state, args).await,
        "query_route_events"     => tools::query_route_events(state, args).await,
        "get_anomalies"          => tools::get_anomalies(state, args).await,
        "get_rpki_status"        => tools::get_rpki_status(state, args).await,
        "get_topology"           => tools::get_topology(state, args).await,
        "compute_igp_path"       => tools::compute_igp_path(state, args).await,
        "test_filter"            => tools::test_filter(state, args).await,
        "get_capacity_status"    => tools::get_capacity_status(state, args).await,
        "natural_language_query" => tools::nl_query(state, args).await,
        "get_convergence_events" => tools::get_convergence_events(state, args).await,
        other => anyhow::bail!("unknown tool: {other}"),
    }
}
```

### 10.4 Natural language → DuckDB SQL

Adapted from bonsai's `nl_query.rs` (NL→Cypher). The DuckDB schema is simpler to describe than bonsai's graph schema.

**System prompt injected to LLM**:
```
You are a BGP network analytics assistant. You convert plain English questions to DuckDB SQL.

Available tables:
  route_events(occurred_at TIMESTAMPTZ, speaker_addr, peer_addr, peer_as UINT,
               rib_type, action, prefix, afi, as_path, as_path_len, origin_asn,
               next_hop, local_pref, med, communities, rpki_validity, collector_id)
  peer_events(occurred_at, speaker_addr, peer_addr, peer_as, event_type,
              reason, hold_time)
  ml_anomalies(detected_at, kind, prefix, peer_addr, score, description, severity)
  path_markings(occurred_at, speaker_addr, peer_addr, prefix, afi,
                path_status UINT, path_reason USMALLINT, status_label, reason_label)
  stats_events(occurred_at, speaker_addr, peer_addr, counter_type, counter_value,
               afi, safi)
  aspa_validations(occurred_at, prefix, peer_addr, customer_asn, result)
  srpolicy_events(occurred_at, speaker_addr, peer_addr, action, endpoint, color, preference)

Rules:
  - Return ONLY the SQL query, no explanation
  - Use TIMESTAMPTZ functions: NOW(), INTERVAL
  - Always include LIMIT (default: 50)
  - Read-only: only SELECT, no INSERT/UPDATE/DELETE
  - Prefix text searches: use ILIKE '%value%'
  - AS numbers are stored as integers

Examples:
  "Show RPKI invalid routes today" →
    SELECT prefix, peer_addr, peer_as, as_path, occurred_at
    FROM route_events
    WHERE rpki_validity = 'invalid' AND occurred_at >= NOW() - INTERVAL '24 hours'
    ORDER BY occurred_at DESC LIMIT 50
```

**Daily token budget**: 500K tokens/day (identical to bonsai). Charged per request. Reset at midnight UTC.

---

## Part 11 — Swagger/OpenAPI Spec

Bonsai has a full 3.0.3 spec with 17 tag groups and inline examples from JSON fixture files. rustybmp should do the same.

### 11.1 Serving pattern (copy from bonsai)

```rust
// In api/mod.rs — add three routes:
.route("/api/openapi.json", get(openapi_json_handler))
.route("/api/swagger",      get(swagger_ui_handler))
.route("/api/resolve",      get(resolve_handler))  // fuzzy entity resolver
```

```rust
pub async fn swagger_ui_handler() -> Html<&'static str> {
    Html(r##"<!DOCTYPE html>
<html lang="en"><head>
  <meta charset="utf-8"/>
  <title>RustyBMP API</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
  <style>body{margin:0}.topbar{display:none}</style>
</head><body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>
window.onload = () => SwaggerUIBundle({
  url: "/api/openapi.json",
  dom_id: "#swagger-ui",
  presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
  layout: "BaseLayout",
  deepLinking: true,
  tryItOutEnabled: true,
  filter: true,
  docExpansion: "none",
  defaultModelsExpandDepth: 2,
});
</script></body></html>"##)
}
```

### 11.2 Tag structure for rustybmp OpenAPI spec

Adapted from bonsai's 17-tag structure:

| Tag | Endpoints |
|-----|-----------|
| **Routing** | /api/routes, /api/routes/prefix/{p}/*, /api/prefixes |
| **Peers** | /api/peers, /api/peers/{addr}/*, /api/speakers |
| **RPKI** | /api/rpki/analysis, /api/rpki/coverage, /api/rpki/stats |
| **Topology** | /api/bgpls/graph, /api/bgpls/path, /api/srpolicy |
| **Policy** | /api/policy, /api/filters/* |
| **Analytics** | /api/aspath/graph, /api/analytics/churn, /api/capacity/maxprefix |
| **ML** | /api/ml/anomalies, /api/ml/model/status, /api/ml/events/stream |
| **BMP Stats** | /api/bmpstats/history, /api/bmp/stats |
| **Path Status** | /api/path-status/matrix (RV7) |
| **Convergence** | /api/convergence (RV7) |
| **Onboarding** | /api/onboard/*, /api/credentials/* |
| **Export** | /api/export/parquet |
| **Policy Fetch** | /api/policy/fetch, /api/policy/configs (RV7) |
| **Output Adapters** | /api/adapters/*, /api/adapters/{name}/test |
| **MCP** | /mcp |
| **SSE** | /api/events |
| **Operations** | /health, /api/metrics, /api/filter/stats |

---

## Part 12 — ML Techniques: Current State and Depth Analysis

### 12.1 What rustybmp currently has

| Model | Location | Features | Output |
|-------|----------|----------|--------|
| Z-score monitor | `analytics.py` | Per-prefix churn rate (IMACSI 2025 eq.2-4) | Z-score, alert if > threshold |
| IsolationForest | `ml/train_route_anomaly.py` | hop_count, is_announce, local_pref, med, community_count, rpki_enc | Anomaly score (-1/1) |
| HijackDetector | `detectors.py` | origin_asn change, prefix specificity | Boolean hijack signal |
| RouteLeakDetector | `detectors.py` | Private ASN in path, OTC attribute | Boolean leak signal |
| FlapScorer | `detectors.py` | Event frequency per peer | Flap count |
| STGNN topology snapshot | `ml/topology_snapshot.py` | Route count, churn rate, RPKI ratio, session uptime | `to_pyg()` stub (incomplete) |

### 12.2 What bonsai has additionally

| Model | Purpose | Architecture |
|-------|---------|-------------|
| IsolationForest | Anomaly detection | Same as rustybmp — baseline |
| STGNN (GATv2-GRU) | Spatio-temporal anomaly on graph | T=8 snapshots, GAT message passing + GRU temporal |
| NCT pre-training | Learn topology structure | Negative Context Training on graph structure |
| Per-event embeddings | Similarity search, dedup | Store in graph for nearest-neighbor lookup |
| Device config embeddings | Config change correlation | NLP embedding of CLI config snapshots |

### 12.3 ML techniques to add in RV8

**Priority 1: Complete the STGNN**

The `to_pyg()` method in `topology_snapshot.py` is a stub. This is the highest-leverage ML completion:

```python
# bmppy/ml/topology_snapshot.py — complete this

def to_pyg(self) -> "HeteroData":
    """Convert to PyTorch Geometric HeteroData for GATv2-GRU training."""
    from torch_geometric.data import HeteroData
    import torch

    data = HeteroData()
    # Node features: route_count, churn_rate_1h, rpki_invalid_ratio,
    #               session_uptime_secs, flap_count_24h
    x = torch.tensor(
        self.nodes_df[NODE_FEATURE_COLS].fillna(0).values,
        dtype=torch.float32
    )
    data['peer'].x = x
    data['peer'].node_id = self.nodes_df['peer_addr'].tolist()

    # Edge features: igp_metric, bandwidth (from bgpls_links)
    if not self.edges_df.empty:
        node_idx = {addr: i for i, addr in enumerate(self.nodes_df['peer_addr'])}
        valid = self.edges_df[
            self.edges_df['src'].isin(node_idx) &
            self.edges_df['dst'].isin(node_idx)
        ]
        src = torch.tensor([node_idx[s] for s in valid['src']], dtype=torch.long)
        dst = torch.tensor([node_idx[d] for d in valid['dst']], dtype=torch.long)
        data['peer', 'sessions_with', 'peer'].edge_index = torch.stack([src, dst])
        efeat = valid[['igp_metric', 'bandwidth']].fillna(0).values
        data['peer', 'sessions_with', 'peer'].edge_attr = torch.tensor(
            efeat, dtype=torch.float32
        )
    return data
```

**Priority 2: Path Status TLV features for IsolationForest**

With RV7's Path Status TLV data, the anomaly model gets much richer features:

```python
# New feature set incorporating Path Status TLV data
PATH_STATUS_FEATURES = [
    "best_count",         # how many peers see this as BEST
    "ecmp_count",         # ECMP paths count
    "backup_count",       # BACKUP paths count
    "filtered_count",     # FILTERED-INBOUND count
    "nonselected_count",  # NON-SELECTED count
    "redundancy_ratio",   # (best+ecmp+backup) / total_peers
    "reason_local_pref_count",  # paths eliminated by LOCAL_PREF
    "reason_as_path_count",     # paths eliminated by AS_PATH length
]

# Anomaly signal: redundancy_ratio drops from 1.0 to 0.2 → loss of ECMP paths
# Anomaly signal: filtered_count spikes → sudden policy filter change
# Anomaly signal: nonselected_count increases → BGP decision instability
```

**Priority 3: Hijack probability classifier**

Replace the simple heuristic `HijackDetector` with a proper classifier:

Features:
- origin_asn_changed (0/1)
- prefix_specificity (new_len - known_len)
- rpki_validity (valid=0, not-found=0.5, invalid=1)
- as_path_len_delta (new - historical_avg)
- aspa_verdict (valid=0, invalid=1, unknown=0.5) — new in RV7
- time_since_last_seen_from_new_origin (hours)
- is_subprefix_of_known (0/1)

Model: LogisticRegression or GradientBoostingClassifier (scikit-learn). Training data: labeled BGP hijack events from BGPStream RIPE historical data.

**Priority 4: Convergence time anomaly**

Is this convergence event taking longer than expected?

```python
class ConvergenceAnomalyDetector:
    """
    Detect abnormally slow BGP convergence using a rolling baseline.

    For each (speaker, peer) pair, maintain a rolling 7-day median of
    convergence_ms. Alert when a new convergence event exceeds 3× the median.
    """
    def check(self, event: ConvergenceEvent) -> Optional[AnomalyAlert]:
        baseline = self._get_baseline(event.peer_addr, days=7)
        if baseline and event.convergence_ms > 3 * baseline.p50:
            return AnomalyAlert(
                kind="slow_convergence",
                peer_addr=event.peer_addr,
                score=event.convergence_ms / baseline.p50,
                description=f"Convergence {event.convergence_ms}ms vs {baseline.p50}ms baseline"
            )
```

**Priority 5: Community semantic inference**

Communities are operator-defined and vendor-specific. But their patterns are discoverable:
- Routes with community `64512:100` consistently have LP=200 → this community probably means "preferred transit"
- Routes with community `65535:666` are consistently absent from Loc-RIB → this community is a blackhole tag

```python
class CommunitySemanticsLearner:
    """
    Learn community semantics from BMP pre/post-policy attribute correlations.
    Uses frequent pattern mining (fpgrowth from mlxtend) to find:
      - Communities that correlate with LP changes
      - Communities that correlate with route rejection (filtered-inbound)
      - Communities that correlate with next-hop changes
    """
    def learn(self, days: int = 30) -> dict[str, CommunityMeaning]:
        ...
```

---

## Part 13 — External Integrations Comparison

### What makes rustybmp unique vs commercial tools

| Capability | Kentik | ThousandEyes | gobmp | OpenBMP | rustybmp |
|-----------|--------|-------------|-------|---------|---------|
| MCP server for AI agents | ❌ | ❌ | ❌ | ❌ | ✅ RV8 |
| Natural language BGP query | ✅ AI | ✅ AI | ❌ | ❌ | ✅ RV8 |
| Elasticsearch output | ✅ | ✅ | ❌ | ✅ | ✅ RV8 |
| Splunk output | ✅ | ✅ | ❌ | ✅ | ✅ RV8 |
| ServiceNow EM output | ❌ | ✅ | ❌ | ❌ | ✅ RV8 |
| Webhook/Slack/PagerDuty | ✅ | ✅ | ❌ | ❌ | ✅ RV8 |
| NetBox enrichment | ❌ | ❌ | ❌ | ❌ | ✅ RV8 |
| ServiceNow CMDB enrichment | ❌ | ❌ | ❌ | ❌ | ✅ RV8 |
| OpenAPI/Swagger | ✅ | ✅ | ❌ | Partial | ✅ RV8 |
| Roto JIT filter language | ❌ | ❌ | ❌ | ❌ | ✅ RV7 |
| Path Status TLV parsing | ❌ | ❌ | ❌ | ❌ | ✅ RV7 |
| Policy pre/post BMP diff | ❌ | ❌ | ❌ | ❌ | ✅ RV6 |

The MCP server is the single clearest differentiator in 2026. ThousandEyes has MCP (announced at Cisco Live 2026 Amsterdam), but rustybmp's MCP would be the only **BMP/BGP-native** MCP server for on-premises networks.

---

## Part 14 — RV8 Epic Index

| Epic | Title | Priority | Copies from bonsai |
|------|-------|----------|--------------------|
| RV8-OA1 | OpenAPI 3.0.3 spec (`api/schema.rs`) | P0 | ✅ schema.rs template |
| RV8-OA2 | Swagger UI served at `/api/swagger` | P0 | ✅ mcp_routes.rs |
| RV8-OA3 | Resolve endpoint (`/api/resolve`) for AI disambiguation | P1 | ✅ mcp_routes.rs |
| RV8-MC1 | MCP server (`/mcp`) with 11 BGP tools | P0 | ✅ mcp_server.rs pattern |
| RV8-MC2 | Natural language → DuckDB SQL endpoint | P1 | ✅ nl_query.rs adapted |
| RV8-MC3 | Daily token budget for NL queries | P1 | ✅ atomic counter pattern |
| RV8-OUT1 | `OutputAdapter` trait + registry | P0 | ✅ traits.rs |
| RV8-OUT2 | Elasticsearch adapter (ECS BGP schema) | P0 | ✅ elastic.rs adapted |
| RV8-OUT3 | Splunk HEC adapter | P0 | ✅ splunk_hec.rs adapted |
| RV8-OUT4 | ServiceNow EM adapter | P1 | ✅ servicenow_em.rs adapted |
| RV8-OUT5 | Webhook adapter (Slack/PagerDuty/OpsGenie profiles) | P1 | 🆕 new |
| RV8-OUT6 | Cursor persistence for all adapters | P0 | ✅ copy dedup + cursor pattern |
| RV8-ENR1 | NetBox enricher (dual REST/MCP transport) | P1 | ✅ netbox.rs + mcp_client.rs |
| RV8-ENR2 | ServiceNow CMDB enricher (router CI context) | P2 | ✅ servicenow.rs adapted |
| RV8-ML1 | Complete `to_pyg()` in topology_snapshot.py | P0 | ✅ bonsai topology_snapshot |
| RV8-ML2 | `train_bgp_stgnn.py` GATv2-GRU training script | P1 | ✅ bonsai train_stgnn.py adapted |
| RV8-ML3 | Path Status TLV features in IsolationForest | P1 | 🆕 new |
| RV8-ML4 | Hijack probability classifier (replace heuristic) | P1 | 🆕 new |
| RV8-ML5 | Convergence anomaly detector | P2 | 🆕 new |
| RV8-ML6 | Community semantics learner | P2 | 🆕 new |
| RV8-UI1 | Output adapter management UI (`/adapters` page) | P1 | 🆕 new |
| RV8-UI2 | Adapter status panel on Dashboard | P1 | 🆕 new |
| RV8-UI3 | NL query interface in UI (`/query` page) | P1 | 🆕 new |

---

## Part 15 — Key Implementation Notes for RV8

### On the OutputAdapter dedup window (from bonsai)

Bonsai's dedup is in-memory: `HashMap<(device_addr, rule_id), last_pushed_ns>`. For rustybmp: `HashMap<(speaker_addr, peer_addr, anomaly_kind), last_pushed_ns>`. The window is configurable per adapter (`dedup_window_secs`). On restart, the dedup window is lost — acceptable because events are deduplicated by the cursor (we won't re-push events already pushed in a previous run; the cursor advances past them).

### On the cursor persistence (from bonsai)

Each adapter writes its cursor (a timestamp) to a `.cursor` file named after the adapter:
```
runtime/cursors/elastic-bgp.cursor
runtime/cursors/splunk-bgp.cursor
runtime/cursors/servicenow-em.cursor
```
On startup, the adapter reads the cursor file and starts polling from that timestamp. On successful push cycle, the cursor advances and is written back. This guarantees at-least-once delivery without duplicates (the dedup window handles the at-most-once part for the same session).

### On the MCP `natural_language_query` tool

The LLM receives the DuckDB schema (compact description, not the full DDL) and the user's question. It returns ONLY the SQL query. The Rust handler validates the query (read-only: no INSERT/UPDATE/DELETE/DROP) and executes it, returning the results as JSON.

The 500K daily token budget from bonsai is appropriate for a production deployment. Track with `AtomicU64` counters (identical to bonsai's implementation), reset at midnight UTC.

### On the MCP server's rule catalogue equivalent

Bonsai has a `RULE_CATALOGUE` with `recurrence_indicators` — Cypher queries that tell an AI agent how to verify whether a detected issue has recurred or resolved. For rustybmp, the equivalent is an `ANOMALY_CATALOGUE` with DuckDB queries:

```rust
pub static ANOMALY_CATALOGUE: &[AnomalyMeta] = &[
    AnomalyMeta {
        kind: "hijack",
        description: "BGP prefix hijack — origin ASN changed to unexpected ASN",
        severity: "critical",
        verification_queries: &[
            "SELECT prefix, origin_asn, occurred_at FROM route_events WHERE prefix = $prefix ORDER BY occurred_at DESC LIMIT 5 — check if origin_asn has returned to expected value",
            "SELECT result FROM aspa_validations WHERE prefix = $prefix ORDER BY occurred_at DESC LIMIT 1 — ASPA verdict should be 'valid' when resolved",
        ],
    },
    // ... more anomaly types
];
```

---

*End of RUSTYBMP_RV8_ANALYSIS.md*
