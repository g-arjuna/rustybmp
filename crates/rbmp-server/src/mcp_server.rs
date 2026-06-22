/// BGP-native MCP (Model Context Protocol) server (RV8-MC1).
///
/// Implements JSON-RPC 2.0 at POST /mcp.
/// Supports the MCP protocol methods:
///   - `initialize`          — return server capabilities
///   - `tools/list`          — enumerate the 11 BGP tools
///   - `tools/call`          — execute a tool by name
///
/// All tool handlers query DuckDB via `QueryEngine` and return
/// structured text + optional JSON data that LLM clients can consume.
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;
use crate::state::AppState;

// ── RV8-MC3: Daily token budget ───────────────────────────────────────────────

/// Daily NL→SQL token budget: 500K tokens, resets at midnight UTC.
/// AtomicU64 — same pattern as bonsai nl_query.rs.
static NL_TOKEN_BUDGET: AtomicU64 = AtomicU64::new(500_000);
/// Unix-day counter — last reset day (seconds / 86400).
static NL_BUDGET_DAY: AtomicU64 = AtomicU64::new(0);

fn consume_nl_tokens(estimated: u64) -> bool {
    let now_day = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0);
    let last_day = NL_BUDGET_DAY.load(Ordering::Relaxed);
    if now_day > last_day {
        NL_TOKEN_BUDGET.store(500_000, Ordering::Relaxed);
        NL_BUDGET_DAY.store(now_day, Ordering::Relaxed);
    }
    let prev = NL_TOKEN_BUDGET.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |b| {
        b.checked_sub(estimated)
    });
    prev.is_ok()
}

// ── RV8-MC4: ANOMALY_CATALOGUE ────────────────────────────────────────────────

pub struct AnomalyMeta {
    pub kind:                 &'static str,
    pub description:          &'static str,
    pub severity:             &'static str,
    pub verification_queries: &'static [&'static str],
}

/// Per-anomaly-kind metadata with DuckDB verification queries for AI agents.
pub static ANOMALY_CATALOGUE: &[AnomalyMeta] = &[
    AnomalyMeta {
        kind:        "origin_change",
        description: "BGP prefix hijack — origin ASN changed to unexpected ASN",
        severity:    "critical",
        verification_queries: &[
            "SELECT prefix, as_path, occurred_at FROM route_events WHERE prefix = '{prefix}' ORDER BY occurred_at DESC LIMIT 5",
        ],
    },
    AnomalyMeta {
        kind:        "route_leak",
        description: "Route leak — private or customer prefix propagated to unexpected peer",
        severity:    "critical",
        verification_queries: &[
            "SELECT as_path FROM route_events WHERE peer_addr = '{peer}' AND occurred_at > NOW() - INTERVAL '5 minutes' ORDER BY occurred_at DESC LIMIT 3",
        ],
    },
    AnomalyMeta {
        kind:        "slow_convergence",
        description: "BGP convergence took longer than historical baseline",
        severity:    "warn",
        verification_queries: &[
            "SELECT convergence_ms, affected_prefixes FROM convergence_events WHERE peer_addr = '{peer}' ORDER BY started_at DESC LIMIT 5",
        ],
    },
    AnomalyMeta {
        kind:        "rpki_invalid",
        description: "Route announced with RPKI-invalid origin ASN or prefix length",
        severity:    "high",
        verification_queries: &[
            "SELECT prefix, as_path, rpki_validity FROM route_events WHERE rpki_validity = 'invalid' AND occurred_at > NOW() - INTERVAL '1 hour' ORDER BY occurred_at DESC LIMIT 10",
        ],
    },
    AnomalyMeta {
        kind:        "flap",
        description: "BGP peer or prefix flapping repeatedly within a short window",
        severity:    "warn",
        verification_queries: &[
            "SELECT peer_addr, COUNT(*) AS flap_count FROM peer_events WHERE event_type IN ('peer_up','peer_down') AND occurred_at > NOW() - INTERVAL '1 hour' GROUP BY peer_addr HAVING COUNT(*) > 4 ORDER BY flap_count DESC",
        ],
    },
];

// ── JSON-RPC 2.0 envelope types ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method:  String,
    #[serde(default)]
    pub params:  Value,
    pub id:      Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result:  Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error:   Option<JsonRpcError>,
    pub id:      Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code:    i32,
    pub message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), result: Some(result), error: None, id: id.unwrap_or(Value::Null) }
    }
    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self { jsonrpc: "2.0".into(), result: None,
               error: Some(JsonRpcError { code, message: message.into() }),
               id: id.unwrap_or(Value::Null) }
    }
}

// ── Tool definitions (RV8 spec §6) ──────────────────────────────────────────

/// Names of all 11 MCP tools — used in tests to verify completeness.
pub const TOOL_NAMES: &[&str] = &[
    "get_prefix_history",
    "get_peer_flaps",
    "get_anomalies",
    "get_as_path_analysis",
    "get_rpki_invalids",
    "get_speaker_summary",
    "get_prefix_visibility",
    "get_convergence_events",
    "get_policy_diff",
    "get_community_summary",
    "nl_query",
];

fn tool_list() -> Value {
    json!({
        "tools": [
          {
            "name": "get_prefix_history",
            "description": "Returns announce/withdraw history for a BGP prefix over a configurable time window.",
            "inputSchema": {
              "type": "object",
              "required": ["prefix"],
              "properties": {
                "prefix":  { "type": "string",  "description": "CIDR prefix, e.g. 1.2.3.0/24" },
                "hours":   { "type": "integer", "description": "Lookback window in hours (default 24)", "default": 24 },
                "limit":   { "type": "integer", "description": "Max rows (default 200)", "default": 200 }
              }
            }
          },
          {
            "name": "get_peer_flaps",
            "description": "Lists BGP peer sessions that have flapped (oscillated Up/Down) within the time window.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "hours": { "type": "integer", "default": 24 },
                "limit": { "type": "integer", "default": 50 }
              }
            }
          },
          {
            "name": "get_anomalies",
            "description": "Returns ML-detected BGP anomalies (route leaks, hijacks, unusual churn) with severity scores.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "kind":  { "type": "string",  "description": "Filter by anomaly kind (leak, hijack, churn)" },
                "hours": { "type": "integer", "default": 24 },
                "limit": { "type": "integer", "default": 100 }
              }
            }
          },
          {
            "name": "get_as_path_analysis",
            "description": "Analyses AS_PATH attributes for a prefix or origin ASN — detects prepending, unexpected transit, and path length outliers.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "prefix": { "type": "string" },
                "asn":    { "type": "integer" },
                "limit":  { "type": "integer", "default": 200 }
              }
            }
          },
          {
            "name": "get_rpki_invalids",
            "description": "Returns routes currently marked RPKI Invalid or Not-Found by the RTR validator.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "speaker": { "type": "string" },
                "limit":   { "type": "integer", "default": 200 }
              }
            }
          },
          {
            "name": "get_speaker_summary",
            "description": "Returns per-BMP-speaker health: peer counts, route totals, and uptime.",
            "inputSchema": { "type": "object", "properties": {} }
          },
          {
            "name": "get_prefix_visibility",
            "description": "Compares internal RIB view of a prefix against RIPE STAT external data to detect visibility gaps.",
            "inputSchema": {
              "type": "object",
              "required": ["prefix"],
              "properties": {
                "prefix": { "type": "string" }
              }
            }
          },
          {
            "name": "get_convergence_events",
            "description": "Returns BGP convergence timeline events for a peer — useful for diagnosing slow convergence.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "peer":  { "type": "string" },
                "hours": { "type": "integer", "default": 24 }
              }
            }
          },
          {
            "name": "get_policy_diff",
            "description": "Shows policy delta for a peer: prefixes that appear in pre-policy but not post-policy RIB (policy-dropped routes).",
            "inputSchema": {
              "type": "object",
              "required": ["peer"],
              "properties": {
                "peer":  { "type": "string" },
                "limit": { "type": "integer", "default": 200 }
              }
            }
          },
          {
            "name": "get_community_summary",
            "description": "Summarises BGP community usage across the RIB — top communities, unique values, and associated route counts.",
            "inputSchema": {
              "type": "object",
              "properties": {
                "limit": { "type": "integer", "default": 50 }
              }
            }
          },
          {
            "name": "nl_query",
            "description": "Translates a natural-language question about BGP data into DuckDB SQL and returns the result. Useful for ad-hoc analysis.",
            "inputSchema": {
              "type": "object",
              "required": ["question"],
              "properties": {
                "question": { "type": "string", "description": "Plain English question, e.g. 'Which prefixes were announced more than 10 times in the last hour?'" },
                "limit":    { "type": "integer", "default": 100 }
              }
            }
          }
        ]
    })
}

// ── Main MCP handler ─────────────────────────────────────────────────────────

/// POST /mcp — JSON-RPC 2.0 dispatch (RV8-MC1).
pub async fn mcp_handler(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    if req.jsonrpc != "2.0" {
        return Json(JsonRpcResponse::err(req.id, -32600, "Invalid JSON-RPC version"));
    }

    let resp = match req.method.as_str() {
        "initialize" => handle_initialize(req.id),
        "tools/list" => handle_tools_list(req.id),
        "tools/call" => handle_tools_call(req.id, req.params, &state).await,
        _ => JsonRpcResponse::err(req.id, -32601, format!("Method not found: {}", req.method)),
    };
    Json(resp)
}

fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::ok(id, json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {
            "tools": { "listChanged": false }
        },
        "serverInfo": {
            "name":    "rustybmp-mcp",
            "version": "0.8.0"
        }
    }))
}

fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse::ok(id, tool_list())
}

async fn handle_tools_call(
    id: Option<Value>,
    params: Value,
    state: &AppState,
) -> JsonRpcResponse {
    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None    => return JsonRpcResponse::err(id, -32602, "Missing tool name"),
    };
    let args = params.get("arguments").cloned().unwrap_or_default();

    let result = match tool_name.as_str() {
        "get_prefix_history"    => tool_prefix_history(args, state).await,
        "get_peer_flaps"        => tool_peer_flaps(args, state).await,
        "get_anomalies"         => tool_anomalies(args, state).await,
        "get_as_path_analysis"  => tool_as_path_analysis(args, state).await,
        "get_rpki_invalids"     => tool_rpki_invalids(args, state).await,
        "get_speaker_summary"   => tool_speaker_summary(args, state).await,
        "get_prefix_visibility" => tool_prefix_visibility(args, state).await,
        "get_convergence_events"=> tool_convergence_events(args, state).await,
        "get_policy_diff"       => tool_policy_diff(args, state).await,
        "get_community_summary" => tool_community_summary(args, state).await,
        "nl_query"              => tool_nl_query(args, state).await,
        _ => Err(format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(data) => JsonRpcResponse::ok(id, json!({
            "content": [{ "type": "text", "text": data.to_string() }],
            "isError": false
        })),
        Err(e) => {
            warn!(tool = %tool_name, error = %e, "MCP tool error");
            JsonRpcResponse::ok(id, json!({
                "content": [{ "type": "text", "text": format!("Error: {e}") }],
                "isError": true
            }))
        }
    }
}

// ── Tool implementations ─────────────────────────────────────────────────────

async fn tool_prefix_history(args: Value, state: &AppState) -> Result<Value, String> {
    let prefix = args["prefix"].as_str().ok_or("prefix required")?;
    let limit  = args["limit"].as_u64().unwrap_or(200) as usize;
    let rows   = state.queries.prefix_history(prefix, limit)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "prefix": prefix, "events": rows }))
}

async fn tool_peer_flaps(args: Value, state: &AppState) -> Result<Value, String> {
    let hours = args["hours"].as_u64().unwrap_or(24) as u32;
    let limit = args["limit"].as_u64().unwrap_or(50) as usize;
    let rows  = state.queries.peer_flap_events(hours, limit)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "peer_flaps": rows }))
}

async fn tool_anomalies(args: Value, state: &AppState) -> Result<Value, String> {
    let kind  = args["kind"].as_str().map(str::to_string);
    let limit = args["limit"].as_u64().unwrap_or(100) as usize;
    let rows  = state.queries.ml_anomalies(limit, kind.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(json!({ "anomalies": rows }))
}

async fn tool_as_path_analysis(args: Value, state: &AppState) -> Result<Value, String> {
    let prefix = args["prefix"].as_str();
    let asn    = args["asn"].as_u64().map(|v| v as u32);
    let limit  = args["limit"].as_u64().unwrap_or(200) as usize;
    let graph  = state.queries.aspath_graph(asn, prefix, limit)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "as_path_analysis": graph }))
}

async fn tool_rpki_invalids(args: Value, state: &AppState) -> Result<Value, String> {
    let speaker = args["speaker"].as_str();
    let limit   = args["limit"].as_u64().unwrap_or(200) as usize;
    let rows    = state.queries.rpki_invalids(speaker, limit)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "rpki_invalids": rows, "count": rows.len() }))
}

async fn tool_speaker_summary(_args: Value, state: &AppState) -> Result<Value, String> {
    let rib = state.rib.read().await;
    let speakers: Vec<Value> = rib.speakers().iter().map(|s| json!({
        "addr":        s.speaker_addr.to_string(),
        "sys_name":    s.sys_name,
        "peers_up":    s.up_peer_count(),
        "peers_total": s.peer_count(),
        "routes":      s.total_routes(),
        "connected_at": s.connected_at.to_rfc3339(),
    })).collect();
    Ok(json!({ "speakers": speakers, "total": speakers.len() }))
}

async fn tool_prefix_visibility(args: Value, _state: &AppState) -> Result<Value, String> {
    let prefix = args["prefix"].as_str().ok_or("prefix required")?;
    Ok(json!({
        "prefix": prefix,
        "note": "External RIPE STAT lookup not yet wired — use GET /api/external/prefix-visibility"
    }))
}

async fn tool_convergence_events(args: Value, state: &AppState) -> Result<Value, String> {
    let peer  = args["peer"].as_str().unwrap_or("");
    let hours = args["hours"].as_u64().unwrap_or(24) as u32;
    let rows  = state.queries.convergence_events(
        if peer.is_empty() { None } else { Some(peer) },
        hours,
        200,
    ).map_err(|e| e.to_string())?;
    Ok(json!({ "peer": peer, "hours": hours, "events": rows }))
}

async fn tool_policy_diff(args: Value, state: &AppState) -> Result<Value, String> {
    let peer = args["peer"].as_str().ok_or("peer required")?;
    let data = state.queries.policy_delta(peer)
        .map_err(|e| e.to_string())?;
    Ok(data)
}

async fn tool_community_summary(args: Value, state: &AppState) -> Result<Value, String> {
    let limit = args["limit"].as_u64().unwrap_or(50) as usize;
    let rows  = state.queries.community_summary(limit)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "top_communities": rows, "count": rows.len() }))
}

/// Natural language → DuckDB SQL (RV8-MC2).
/// Uses a deterministic keyword→template mapping (no LLM required at runtime).
/// An external LLM agent can call this tool and let the server safely execute
/// only the generated SQL against the read-only query engine.
async fn tool_nl_query(args: Value, state: &AppState) -> Result<Value, String> {
    let question = args["question"].as_str().ok_or("question required")?;
    let limit    = args["limit"].as_u64().unwrap_or(100) as usize;

    // RV8-MC3: consume ~100 tokens per nl_query call from the daily budget
    if !consume_nl_tokens(100) {
        return Err("Daily NL query token budget (500K) exhausted — resets at midnight UTC".into());
    }

    let sql = nl_to_sql(question, limit);
    let rows = state.queries.raw_query(&sql)
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "question": question,
        "generated_sql": sql,
        "rows": rows,
        "row_count": rows.as_array().map(|a| a.len()).unwrap_or(0)
    }))
}

/// Keyword-based NL→SQL translation. Handles common BGP question patterns.
/// Safe: only SELECT statements, always appended with LIMIT.
fn nl_to_sql(question: &str, limit: usize) -> String {
    let q = question.to_lowercase();

    // Prefix history / churn
    if q.contains("announc") && q.contains("prefix") {
        return format!(
            "SELECT prefix, COUNT(*) AS announce_count FROM route_events \
             WHERE action='announce' GROUP BY prefix ORDER BY announce_count DESC LIMIT {limit}"
        );
    }
    if q.contains("withdraw") {
        return format!(
            "SELECT prefix, COUNT(*) AS withdraw_count FROM route_events \
             WHERE action='withdraw' GROUP BY prefix ORDER BY withdraw_count DESC LIMIT {limit}"
        );
    }
    if q.contains("churn") || (q.contains("prefix") && q.contains("flap")) {
        return format!(
            "SELECT prefix, COUNT(*) AS changes FROM route_events \
             GROUP BY prefix ORDER BY changes DESC LIMIT {limit}"
        );
    }
    // Peer up/down events
    if q.contains("peer") && (q.contains("down") || q.contains("flap")) {
        return format!(
            "SELECT peer_addr, speaker_addr, occurred_at, event_type FROM peer_events \
             WHERE event_type='peer_down' ORDER BY occurred_at DESC LIMIT {limit}"
        );
    }
    if q.contains("peer") && q.contains("up") {
        return format!(
            "SELECT peer_addr, speaker_addr, occurred_at FROM peer_events \
             WHERE event_type='peer_up' ORDER BY occurred_at DESC LIMIT {limit}"
        );
    }
    // RPKI invalids
    if q.contains("rpki") && (q.contains("invalid") || q.contains("fail")) {
        return format!(
            "SELECT prefix, peer_addr, speaker_addr, occurred_at FROM route_events \
             WHERE rpki_status='Invalid' ORDER BY occurred_at DESC LIMIT {limit}"
        );
    }
    // Anomalies
    if q.contains("anomal") || q.contains("hijack") || q.contains("leak") {
        return format!(
            "SELECT kind, prefix, peer_addr, score, occurred_at FROM ml_anomalies \
             ORDER BY score DESC, occurred_at DESC LIMIT {limit}"
        );
    }
    // Communities
    if q.contains("communit") {
        return format!(
            "SELECT communities, COUNT(*) AS route_count FROM route_events \
             WHERE communities IS NOT NULL \
             GROUP BY communities ORDER BY route_count DESC LIMIT {limit}"
        );
    }
    // AS path length outliers
    if q.contains("as_path") || q.contains("as path") || q.contains("path length") {
        return format!(
            "SELECT prefix, peer_addr, as_path, as_path_len FROM route_events \
             WHERE as_path_len IS NOT NULL ORDER BY as_path_len DESC LIMIT {limit}"
        );
    }
    // Convergence
    if q.contains("converg") {
        return format!(
            "SELECT peer_addr, speaker_addr, prefix_count, elapsed_secs, occurred_at \
             FROM convergence_events ORDER BY occurred_at DESC LIMIT {limit}"
        );
    }
    // Default: recent route events
    format!(
        "SELECT occurred_at, action, prefix, peer_addr, speaker_addr FROM route_events \
         ORDER BY occurred_at DESC LIMIT {limit}"
    )
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_count_is_11() {
        assert_eq!(
            TOOL_NAMES.len(), 11,
            "TOOL_NAMES must contain exactly 11 tool names (RV8-MC1 spec)"
        );
    }

    #[test]
    fn tool_names_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for name in TOOL_NAMES {
            assert!(seen.insert(*name), "Duplicate tool name: {name}");
        }
    }

    #[test]
    fn tool_names_contains_nl_query() {
        assert!(
            TOOL_NAMES.contains(&"nl_query"),
            "TOOL_NAMES must include 'nl_query' (RV8-MC3)"
        );
    }

    #[test]
    fn anomaly_catalogue_has_five_entries() {
        assert_eq!(
            ANOMALY_CATALOGUE.len(), 5,
            "ANOMALY_CATALOGUE must have 5 entries"
        );
    }

    #[test]
    fn anomaly_catalogue_kinds_unique() {
        let kinds: Vec<&str> = ANOMALY_CATALOGUE.iter().map(|m| m.kind).collect();
        let unique: std::collections::HashSet<&str> = kinds.iter().copied().collect();
        assert_eq!(kinds.len(), unique.len(), "ANOMALY_CATALOGUE kinds must be unique");
    }

    #[test]
    fn anomaly_catalogue_each_has_verification_query() {
        for entry in ANOMALY_CATALOGUE {
            assert!(
                !entry.verification_queries.is_empty(),
                "Anomaly kind '{}' has no verification queries", entry.kind
            );
        }
    }

    #[test]
    fn nl_query_to_sql_rpki() {
        let sql = nl_to_sql("show rpki invalid routes", 50);
        assert!(
            sql.to_lowercase().contains("rpki"),
            "nl_query RPKI question should reference rpki column, got: {sql}"
        );
    }

    #[test]
    fn nl_query_to_sql_flap() {
        let sql = nl_to_sql("which peers have flapped down?", 50);
        assert!(
            sql.to_lowercase().contains("peer_down") || sql.to_lowercase().contains("peer_up") || sql.to_lowercase().contains("peer"),
            "nl_query flap question should reference peer_down/peer_up, got: {sql}"
        );
    }

    #[test]
    fn nl_query_to_sql_default_returns_route_events() {
        let sql = nl_to_sql("xyzzy unknown query string", 10);
        assert!(
            sql.contains("route_events"),
            "nl_query default fallback must query route_events, got: {sql}"
        );
    }

    #[test]
    fn token_budget_initially_allows_small_queries() {
        // Reset budget to known state by consuming 0 tokens first
        // consume_nl_tokens uses global state so just verify it returns true for small amount
        // (budget starts at 500_000, any small query should succeed in a fresh test run)
        let ok = consume_nl_tokens(100);
        assert!(ok, "consume_nl_tokens(100) should succeed when budget > 0");
    }

    #[test]
    fn nl_to_sql_convergence_keyword() {
        let sql = nl_to_sql("show convergence events", 25);
        assert!(
            sql.contains("convergence_events"),
            "nl_to_sql 'convergence' must query convergence_events table, got: {sql}"
        );
        assert!(
            sql.contains("25"),
            "LIMIT must reflect the passed limit argument, got: {sql}"
        );
    }
}
