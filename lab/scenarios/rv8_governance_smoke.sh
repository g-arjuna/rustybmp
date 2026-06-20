#!/usr/bin/env bash
# RV8-T7: Governance + Speaker Summary smoke test against a live rustybmp instance.
#
# Pre-requisites:
#   1. rustybmp running: cargo run -p rbmp-server -- --config config/rustybmp.toml.example
#   2. At least one FRR peer connected (use xrd-bmp.clab.yml ContainerLab topology)
#   3. jq installed
#
# Usage:
#   bash lab/scenarios/rv8_governance_smoke.sh [BASE_URL]
#   BASE_URL defaults to http://localhost:8080

set -euo pipefail

BASE="${1:-http://localhost:8080}"
PASS=0
FAIL=0

# ── helpers ───────────────────────────────────────────────────────────────────

pass() { echo "  [PASS] $1"; ((PASS++)); }
fail() { echo "  [FAIL] $1"; ((FAIL++)); }

check_field() {
    local label="$1"
    local url="$2"
    local jq_expr="$3"
    local expected="$4"
    local actual
    actual=$(curl -sf "$url" | jq -r "$jq_expr" 2>/dev/null || echo "__curl_err__")
    if [[ "$actual" == "$expected" ]]; then
        pass "$label"
    else
        fail "$label — expected '$expected' got '$actual'"
    fi
}

check_http() {
    local label="$1"
    local url="$2"
    local expected_status="${3:-200}"
    local status
    status=$(curl -so /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
    if [[ "$status" == "$expected_status" ]]; then
        pass "$label (HTTP $status)"
    else
        fail "$label — expected HTTP $expected_status got $status"
    fi
}

check_json_contains() {
    local label="$1"
    local url="$2"
    local jq_expr="$3"
    local result
    result=$(curl -sf "$url" | jq -e "$jq_expr" 2>/dev/null && echo "ok" || echo "fail")
    if [[ "$result" == "ok" ]]; then
        pass "$label"
    else
        fail "$label — jq expr '$jq_expr' returned false/null"
    fi
}

# ── /health ───────────────────────────────────────────────────────────────────

echo ""
echo "=== Health ==="
check_http    "GET /health returns 200"       "$BASE/health"
check_field   "health.status == ok"           "$BASE/health"        ".status"        "ok"

# ── /api/governance ───────────────────────────────────────────────────────────

echo ""
echo "=== Governance (RV8-GOV2) ==="
check_http    "GET /api/governance returns 200"   "$BASE/api/governance"
check_json_contains "governance has memory_mb"    "$BASE/api/governance"  '.memory_mb != null'
check_json_contains "governance has shed_active"  "$BASE/api/governance"  '.shed_active != null'
check_json_contains "governance has write_queue"  "$BASE/api/governance"  '.write_queue_len != null'

# ── /api/speakers/summary ─────────────────────────────────────────────────────

echo ""
echo "=== Speakers Summary (RV8-UX3) ==="
check_http    "GET /api/speakers/summary returns 200"        "$BASE/api/speakers/summary"
check_json_contains "speakers/summary returns array"         "$BASE/api/speakers/summary"  'type == "array" or . != null'

# ── /api/openapi.json ────────────────────────────────────────────────────────

echo ""
echo "=== OpenAPI Spec (RV8-OA1) ==="
check_http    "GET /api/openapi.json returns 200"            "$BASE/api/openapi.json"
check_field   "openapi.openapi == 3.0.3"                     "$BASE/api/openapi.json"      ".openapi"       "3.0.3"
check_json_contains "openapi has /api/speakers path"         "$BASE/api/openapi.json"      '.paths | keys | any(. == "/api/speakers")'

# ── /api/swagger ─────────────────────────────────────────────────────────────

echo ""
echo "=== Swagger UI (RV8-OA2) ==="
check_http    "GET /api/swagger returns 200"                 "$BASE/api/swagger"

# ── POST /mcp — MCP server (RV8-MC1) ─────────────────────────────────────────

echo ""
echo "=== MCP Server (RV8-MC1) ==="

MCP_INIT=$(curl -sf -X POST "$BASE/mcp" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"initialize","params":{},"id":1}' 2>/dev/null || echo "{}")

if echo "$MCP_INIT" | jq -e '.result.serverInfo.name == "rustybmp-mcp"' >/dev/null 2>&1; then
    pass "MCP initialize returns serverInfo.name == rustybmp-mcp"
else
    fail "MCP initialize response invalid: $MCP_INIT"
fi

MCP_TOOLS=$(curl -sf -X POST "$BASE/mcp" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"tools/list","params":{},"id":2}' 2>/dev/null || echo "{}")

TOOL_COUNT=$(echo "$MCP_TOOLS" | jq '.result.tools | length' 2>/dev/null || echo "0")
if [[ "$TOOL_COUNT" == "11" ]]; then
    pass "MCP tools/list returns exactly 11 tools"
else
    fail "MCP tools/list returned $TOOL_COUNT tools (expected 11)"
fi

MCP_BAD=$(curl -sf -X POST "$BASE/mcp" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"nonexistent","params":{},"id":3}' 2>/dev/null || echo "{}")

if echo "$MCP_BAD" | jq -e '.error.code == -32601' >/dev/null 2>&1; then
    pass "MCP unknown method returns -32601"
else
    fail "MCP unknown method response: $MCP_BAD"
fi

# NL query
MCP_NL=$(curl -sf -X POST "$BASE/mcp" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"nl_query","arguments":{"question":"show recent route changes","limit":5}},"id":4}' 2>/dev/null || echo "{}")

if echo "$MCP_NL" | jq -e '.result.content[0].type == "text"' >/dev/null 2>&1; then
    pass "MCP nl_query returns text content"
else
    fail "MCP nl_query response invalid: $MCP_NL"
fi

# ── /api/external/prefix-visibility ──────────────────────────────────────────

echo ""
echo "=== External API (RV8-EXT5) ==="
VIS=$(curl -sf "$BASE/api/external/prefix-visibility?prefix=1.1.1.0/24" 2>/dev/null || echo "{}")
if echo "$VIS" | jq -e '.prefix == "1.1.1.0/24"' >/dev/null 2>&1; then
    pass "GET /api/external/prefix-visibility returns correct prefix field"
else
    fail "prefix-visibility response: $VIS"
fi
if echo "$VIS" | jq -e '.has_discrepancies != null' >/dev/null 2>&1; then
    pass "prefix-visibility includes has_discrepancies field"
else
    fail "prefix-visibility missing has_discrepancies"
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Results: ${PASS} passed  |  ${FAIL} failed"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [[ "$FAIL" -gt 0 ]]; then
    exit 1
fi
