#!/usr/bin/env bash
# Simulate a BGP peer flap on CE1 by bouncing FRR's bgpd.
# Usage: ./flap_peer.sh [iterations] [down_secs] [up_wait_secs]
set -euo pipefail

CONTAINER="${CLAB_NODE:-clab-xrd-bmp-ce1}"
ITERATIONS="${1:-3}"
DOWN_SECS="${2:-5}"
UP_WAIT_SECS="${3:-15}"

echo "==> BGP peer flap: $ITERATIONS iterations, ${DOWN_SECS}s down, ${UP_WAIT_SECS}s recovery"

for i in $(seq 1 "$ITERATIONS"); do
    echo "[${i}/${ITERATIONS}] Stopping bgpd on ${CONTAINER}"
    docker exec "$CONTAINER" killall bgpd || true
    sleep "$DOWN_SECS"

    echo "[${i}/${ITERATIONS}] Restarting FRR"
    docker exec "$CONTAINER" /usr/lib/frr/frrinit.sh start
    echo "[${i}/${ITERATIONS}] Waiting ${UP_WAIT_SECS}s for session to re-establish"
    sleep "$UP_WAIT_SECS"
done

echo "==> Flap sequence complete"
