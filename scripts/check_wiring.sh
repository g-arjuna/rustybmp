#!/usr/bin/env bash
# Bundle A2 — Layer 1 wiring checks: fast structural validation (<15s, no build needed)
# Run from the repository root:  bash scripts/check_wiring.sh
set -euo pipefail

ERRORS=0
PASS=0

check() {
    local desc="$1"
    local cmd="$2"
    if eval "$cmd" &>/dev/null; then
        echo "PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "FAIL: $desc"
        ERRORS=$((ERRORS + 1))
    fi
}

# ── API module wiring ─────────────────────────────────────────────────────────
check "api/filters wired in mod.rs" \
    "grep -q 'pub mod filters' crates/rbmp-server/src/api/mod.rs"

check "api/governance wired in mod.rs" \
    "grep -q 'pub mod governance' crates/rbmp-server/src/api/mod.rs"

check "api/external wired in mod.rs" \
    "grep -q 'pub mod external' crates/rbmp-server/src/api/mod.rs"

check "api/schema (swagger) wired in mod.rs" \
    "grep -q 'pub mod schema' crates/rbmp-server/src/api/mod.rs"

# ── MCP server ───────────────────────────────────────────────────────────────
check "mcp_server module referenced in main.rs" \
    "grep -q 'mcp_server' crates/rbmp-server/src/main.rs"

check "mcp TOOL_NAMES has >= 11 entries" \
    "awk '/^pub const TOOL_NAMES/,/^];/' crates/rbmp-server/src/mcp_server.rs | grep -c '\"' | xargs -I{} test {} -ge 11"

# ── Output adapters ───────────────────────────────────────────────────────────
check "output/elasticsearch.rs exists" \
    "test -f crates/rbmp-server/src/output/elasticsearch.rs"

check "output/splunk.rs exists" \
    "test -f crates/rbmp-server/src/output/splunk.rs"

# ── No bonsai leakage ────────────────────────────────────────────────────────
check "no BONSAI_VAULT_PASSPHRASE used as env var in crates/" \
    "! grep -rn 'BONSAI_VAULT_PASSPHRASE' crates/ | grep -v '^\s*//' | grep -qv '///'"

check "no BONSAI_BOOTSTRAP_ in bmppy/" \
    "! grep -rq 'BONSAI_BOOTSTRAP_' bmppy/"

# ── Config / asset files ─────────────────────────────────────────────────────
check "config/filters.roto exists" \
    "test -f config/filters.roto"

check "config/filters.yaml exists" \
    "test -f config/filters.yaml"

check "tests/seed.sql exists" \
    "test -f tests/seed.sql"

# ── Python modules ───────────────────────────────────────────────────────────
check "bmppy/rbmppy/internet.py has RipeStatClient" \
    "grep -q 'class RipeStatClient' bmppy/rbmppy/internet.py"

check "bmppy/policy_fetcher.py is valid Python syntax" \
    "python3 -c 'import ast; ast.parse(open(\"bmppy/policy_fetcher.py\").read())'"

# ── Summary ───────────────────────────────────────────────────────────────────
TOTAL=$((PASS + ERRORS))
echo ""
echo "Wiring checks: $PASS/$TOTAL passed"

if [ "$ERRORS" -gt 0 ]; then
    echo "WIRING FAILED: $ERRORS check(s) failed"
    exit 1
fi

echo "All wiring checks passed"
