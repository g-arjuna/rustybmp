#!/usr/bin/env bash
# Trigger a mass prefix withdrawal by removing all network statements
# from CE1's FRR config, then re-announcing after a delay.
# Usage: ./mass_withdrawal.sh [down_secs]
set -euo pipefail

CONTAINER="${CLAB_NODE:-clab-xrd-bmp-ce1}"
DOWN_SECS="${1:-30}"

PREFIXES=(
    "192.0.2.0/24"
    "192.0.2.128/25"
    "198.51.100.0/24"
)

withdraw() {
    echo "==> Withdrawing ${#PREFIXES[@]} prefixes"
    for pfx in "${PREFIXES[@]}"; do
        docker exec "$CONTAINER" vtysh -c "conf t" \
            -c "router bgp 65001" \
            -c "address-family ipv4 unicast" \
            -c "no network ${pfx}"
    done
    docker exec "$CONTAINER" vtysh -c "clear ip bgp * soft out"
}

announce() {
    echo "==> Re-announcing ${#PREFIXES[@]} prefixes"
    for pfx in "${PREFIXES[@]}"; do
        docker exec "$CONTAINER" vtysh -c "conf t" \
            -c "router bgp 65001" \
            -c "address-family ipv4 unicast" \
            -c "network ${pfx}"
    done
    docker exec "$CONTAINER" vtysh -c "clear ip bgp * soft out"
}

withdraw
echo "==> Holding down for ${DOWN_SECS}s"
sleep "$DOWN_SECS"
announce
echo "==> Mass withdrawal scenario complete"
