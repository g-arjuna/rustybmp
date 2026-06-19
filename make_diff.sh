#!/usr/bin/env bash
# scripts/dev/make_diff.sh
#
# PURPOSE: Generate a minimal diff patch for uploading to Claude in the next session.
# Run this from the repo root after you've committed your RV1 coding session.
#
# USAGE:
#   ./scripts/dev/make_diff.sh                  # auto-detect last tag
#   ./scripts/dev/make_diff.sh abc1234          # from specific commit
#   ./scripts/dev/make_diff.sh abc1234 def5678  # specific range
#
# OUTPUT: diffs/diff_YYYYMMDD_HHMM_<from>_<to>.patch
#
# UPLOAD: The .patch file + RUSTYBMP_PROJECT_CONTEXT.md is all Claude needs.
#         Do NOT upload the full zip again.

set -euo pipefail

FROM=${1:-$(git describe --tags --abbrev=0 2>/dev/null || git rev-list --max-parents=0 HEAD)}
TO=${2:-HEAD}
TIMESTAMP=$(date +%Y%m%d_%H%M)
FROM_SHORT=${FROM:0:8}
TO_SHORT=$(git rev-parse --short "$TO")
OUT="diffs/diff_${TIMESTAMP}_${FROM_SHORT}_${TO_SHORT}.patch"

mkdir -p diffs

echo "Generating diff from $FROM to $TO..."

git diff "$FROM" "$TO" -- \
    'crates/**/*.rs' \
    'bmppy/**/*.py' \
    'bmppy/pyproject.toml' \
    'lab/**/*.yml' \
    'lab/**/*.cfg' \
    'lab/**/*.sh' \
    'lab/**/*.yaml' \
    'scripts/**/*.sh' \
    'config/**' \
    'Cargo.toml' \
    'Cargo.lock' \
    > "$OUT"

echo ""
echo "=== Diff written to: $OUT ==="
echo ""
echo "=== Files changed ==="
git diff --name-only "$FROM" "$TO"
echo ""
echo "=== Stats ==="
echo "Lines added:   $(grep '^+' "$OUT" | grep -v '^+++' | wc -l)"
echo "Lines removed: $(grep '^-' "$OUT" | grep -v '^---' | wc -l)"
echo "Patch size:    $(wc -c < "$OUT") bytes"
echo ""
echo "=== Cargo test status ==="
cargo test --workspace --quiet 2>&1 | tail -5
echo ""
echo "=== Upload to Claude ==="
echo "1. $OUT"
echo "2. RUSTYBMP_PROJECT_CONTEXT.md (from project files)"
echo ""
echo "Tell Claude: 'Here is the RV1 diff. Mark completed tasks and generate RUSTYBMP_BACKLOG_RV2.md'"
