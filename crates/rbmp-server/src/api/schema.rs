/// OpenAPI 3.0.3 specification for the rustybmp API (RV8-OA1).
///
/// Served at:
///   GET /api/openapi.json   — raw spec
///   GET /api/swagger        — Swagger UI (HTML, pulls spec from above)
use axum::{response::Html, Json};
use serde_json::{json, Value};

/// GET /api/openapi.json
pub async fn openapi_spec() -> Json<Value> {
    Json(build_spec())
}

/// GET /api/swagger  — inline Swagger UI using CDN assets
pub async fn swagger_ui() -> Html<String> {
    Html(r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <title>rustybmp API</title>
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    SwaggerUIBundle({
      url: '/api/openapi.json',
      dom_id: '#swagger-ui',
      presets: [SwaggerUIBundle.presets.apis, SwaggerUIBundle.SwaggerUIStandalonePreset],
      layout: 'StandaloneLayout',
      deepLinking: true,
      tryItOutEnabled: true,
    });
  </script>
</body>
</html>"#.to_string())
}

fn build_spec() -> Value {
    use serde_json::Map;

    let mut spec = Map::new();
    spec.insert("openapi".into(), json!("3.0.3"));
    spec.insert("info".into(), json!({
        "title": "rustybmp API",
        "description": "BGP Monitoring Protocol collector, RIB, analytics, and policy engine.",
        "version": "0.8.0",
        "contact": { "name": "rustybmp" },
        "license": { "name": "MIT" }
    }));
    spec.insert("servers".into(), json!([{ "url": "/", "description": "Current server" }]));
    spec.insert("security".into(), json!([{ "BearerAuth": [] }]));
    spec.insert("tags".into(), build_tags());
    spec.insert("components".into(), build_components());
    spec.insert("paths".into(), build_paths());
    Value::Object(spec)
}

fn build_tags() -> Value {
    json!([
        { "name": "health",      "description": "Health and readiness probes" },
        { "name": "speakers",    "description": "BMP speaker management" },
        { "name": "peers",       "description": "BGP peer sessions" },
        { "name": "routes",      "description": "Route table queries" },
        { "name": "analytics",   "description": "BGP analytics and churn" },
        { "name": "rpki",        "description": "RPKI validation and coverage" },
        { "name": "ml",          "description": "ML anomaly detection" },
        { "name": "topology",    "description": "BGP-LS topology graph" },
        { "name": "capacity",    "description": "Max-prefix capacity gauges" },
        { "name": "filters",     "description": "Roto/YAML filter management" },
        { "name": "path_status", "description": "RFC 9069 path status TLV" },
        { "name": "credentials", "description": "SSH credential vault" },
        { "name": "governance",  "description": "Resource governor status" },
        { "name": "onboarding",  "description": "Speaker onboarding" },
        { "name": "mcp",         "description": "BGP MCP server (JSON-RPC 2.0)" }
    ])
}

fn build_components() -> Value {
    json!({
        "securitySchemes": {
            "BearerAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" }
        },
        "schemas": {
            "Error": {
                "type": "object",
                "properties": { "error": { "type": "string" } }
            },
            "GovernanceSnapshot": {
                "type": "object",
                "properties": {
                    "profile":                  { "type": "string" },
                    "memory_budget_mb":         { "type": "integer" },
                    "rate_budget_eps":          { "type": "integer" },
                    "memory_pressure_active":   { "type": "boolean" },
                    "write_pressure_active":    { "type": "boolean" },
                    "rate_shedding_active":     { "type": "boolean" },
                    "memory_shrink_count":      { "type": "integer" },
                    "write_batch_expand_count": { "type": "integer" },
                    "rate_shed_count":          { "type": "integer" }
                }
            },
            "SpeakerSummaryRow": {
                "type": "object",
                "properties": {
                    "addr":         { "type": "string" },
                    "hostname":     { "type": "string" },
                    "vendor":       { "type": "string" },
                    "bmp_state":    { "type": "string", "enum": ["active", "idle"] },
                    "peers_up":     { "type": "integer" },
                    "peers_down":   { "type": "integer" },
                    "route_count":  { "type": "integer" },
                    "connected_at": { "type": "string", "format": "date-time" }
                }
            }
        }
    })
}

fn build_paths() -> Value {
    let mut paths = serde_json::Map::new();

    paths.insert("/health".into(), json!({
        "get": { "tags": ["health"], "summary": "Liveness probe", "security": [],
            "responses": { "200": { "description": "Service is alive" } } }
    }));
    paths.insert("/api/speakers".into(), json!({
        "get": { "tags": ["speakers"], "summary": "List all BMP speakers",
            "responses": { "200": { "description": "Speaker list" } } }
    }));
    paths.insert("/api/speakers/summary".into(), json!({
        "get": { "tags": ["speakers"], "summary": "Aggregated per-speaker summary (adaptive homepage)",
            "responses": { "200": { "description": "Speaker summary" } } }
    }));
    paths.insert("/api/peers".into(), json!({
        "get": { "tags": ["peers"], "summary": "List all BGP peers",
            "responses": { "200": { "description": "Peer list" } } }
    }));
    paths.insert("/api/routes".into(), json!({
        "get": { "tags": ["routes"], "summary": "Query route events",
            "parameters": [
                { "name": "speaker", "in": "query", "schema": { "type": "string" } },
                { "name": "peer",    "in": "query", "schema": { "type": "string" } },
                { "name": "prefix",  "in": "query", "schema": { "type": "string" } },
                { "name": "action",  "in": "query", "schema": { "type": "string" } },
                { "name": "hours",   "in": "query", "schema": { "type": "integer", "default": 24 } },
                { "name": "limit",   "in": "query", "schema": { "type": "integer", "default": 200 } }
            ],
            "responses": { "200": { "description": "Route rows" } } }
    }));
    paths.insert("/api/analytics/churn".into(), json!({
        "get": { "tags": ["analytics"], "summary": "Top churning prefixes",
            "responses": { "200": { "description": "Churn data" } } }
    }));
    paths.insert("/api/rpki/stats".into(), json!({
        "get": { "tags": ["rpki"], "summary": "RPKI validation stats",
            "responses": { "200": { "description": "Stats" } } }
    }));
    paths.insert("/api/rpki/coverage".into(), json!({
        "get": { "tags": ["rpki"], "summary": "RPKI coverage breakdown",
            "responses": { "200": { "description": "Coverage" } } }
    }));
    paths.insert("/api/ml/anomalies".into(), json!({
        "get": { "tags": ["ml"], "summary": "ML-detected BGP anomalies",
            "parameters": [
                { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 100 } },
                { "name": "kind",  "in": "query", "schema": { "type": "string" } }
            ],
            "responses": { "200": { "description": "Anomaly list" } } }
    }));
    paths.insert("/api/bgpls/graph".into(), json!({
        "get": { "tags": ["topology"], "summary": "BGP-LS topology graph",
            "parameters": [
                { "name": "protocol", "in": "query", "schema": { "type": "string" } }
            ],
            "responses": { "200": { "description": "Graph nodes and edges" } } }
    }));
    paths.insert("/api/capacity/max-prefix".into(), json!({
        "get":  { "tags": ["capacity"], "summary": "List max-prefix capacities",
            "responses": { "200": { "description": "Rows" } } },
        "post": { "tags": ["capacity"], "summary": "Upsert max-prefix capacity",
            "responses": { "200": { "description": "Updated" } } }
    }));
    paths.insert("/api/governance".into(), json!({
        "get": { "tags": ["governance"], "summary": "Resource governor current state",
            "responses": { "200": { "description": "Governor snapshot",
                "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/GovernanceSnapshot" }
                } } } } }
    }));
    paths.insert("/api/filters/stats".into(), json!({
        "get": { "tags": ["filters"], "summary": "Filter engine statistics",
            "responses": { "200": { "description": "Stats" } } }
    }));
    paths.insert("/api/filters/reload".into(), json!({
        "post": { "tags": ["filters"], "summary": "Hot-reload the filter file",
            "responses": { "200": { "description": "Reloaded" } } }
    }));
    paths.insert("/api/credentials".into(), json!({
        "get":  { "tags": ["credentials"], "summary": "List credential aliases",
            "responses": { "200": { "description": "List" } } },
        "post": { "tags": ["credentials"], "summary": "Add a credential",
            "responses": { "201": { "description": "Created" } } }
    }));
    paths.insert("/api/path-status/matrix".into(), json!({
        "get": { "tags": ["path_status"], "summary": "RFC 9069 path status matrix",
            "responses": { "200": { "description": "Matrix" } } }
    }));
    paths.insert("/api/convergence".into(), json!({
        "get": { "tags": ["analytics"], "summary": "BGP convergence events",
            "parameters": [
                { "name": "peer",  "in": "query", "schema": { "type": "string" } },
                { "name": "hours", "in": "query", "schema": { "type": "integer", "default": 24 } }
            ],
            "responses": { "200": { "description": "Events" } } }
    }));
    paths.insert("/api/onboard/speaker".into(), json!({
        "post": { "tags": ["onboarding"], "summary": "Register a new BMP speaker",
            "responses": { "200": { "description": "Registered" } } }
    }));
    paths.insert("/mcp".into(), json!({
        "post": { "tags": ["mcp"], "summary": "BGP MCP JSON-RPC 2.0 — 11 BGP tools",
            "security": [],
            "requestBody": { "required": true, "content": { "application/json": {
                "schema": { "type": "object", "properties": {
                    "jsonrpc": { "type": "string", "example": "2.0" },
                    "method":  { "type": "string", "example": "tools/list" },
                    "id":      { "type": "integer" }
                } }
            } } },
            "responses": { "200": { "description": "JSON-RPC response" } } }
    }));

    Value::Object(paths)
}
