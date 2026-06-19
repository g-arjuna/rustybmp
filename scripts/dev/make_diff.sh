#!/usr/bin/env bash
# Generate a git diff suitable for review — excludes generated files and lock files.
set -euo pipefail

BASE="${1:-main}"

git diff "${BASE}...HEAD" \
    -- \
    ':(exclude)Cargo.lock' \
    ':(exclude)*.lock' \
    ':(exclude)target/' \
    ':(exclude).clab/'
