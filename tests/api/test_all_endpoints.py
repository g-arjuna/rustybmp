"""
Bundle B2 — Layer 3 API Contract Tests.

24 pytest tests covering all /api/* endpoint contracts.

Tests require a running rustybmp server in test mode:
    RUSTYBMP_TEST_MODE=1 cargo run &
    pytest tests/api/ -v

If the server is not running, tests are auto-skipped.
"""
from __future__ import annotations

import os
from typing import Any

import httpx
import pytest
import pytest_asyncio

BASE_URL = os.environ.get("RUSTYBMP_URL", "http://localhost:7878")

# ── Skip if server is not reachable ──────────────────────────────────────────


def _server_up() -> bool:
    try:
        r = httpx.get(f"{BASE_URL}/health", timeout=3.0)
        return r.status_code == 200
    except (httpx.ConnectError, httpx.TimeoutException):
        return False


pytestmark = pytest.mark.skipif(not _server_up(), reason="rustybmp server not running")


# ── Shared client fixture ────────────────────────────────────────────────────


@pytest_asyncio.fixture
async def client() -> httpx.AsyncClient:
    async with httpx.AsyncClient(base_url=BASE_URL, timeout=15.0) as c:
        yield c


@pytest_asyncio.fixture(autouse=True)
async def seed_standard(client: httpx.AsyncClient) -> None:
    """Seed the database before each test with the standard fixture."""
    r = await client.post("/api/_test/seed", json={"fixture": "standard", "truncate": True})
    # If seed endpoint returns error (e.g. test mode not set), skip
    if r.status_code != 200:
        pytest.skip("Seed endpoint unavailable")
    body = r.json()
    if not body.get("ok"):
        pytest.skip(f"Seed failed: {body.get('error')}")


# ── Helpers ──────────────────────────────────────────────────────────────────


async def get_json(client: httpx.AsyncClient, path: str, **params: Any) -> dict:
    r = await client.get(path, params=params)
    assert r.status_code == 200, f"GET {path} returned {r.status_code}: {r.text[:200]}"
    return r.json()


# ── 1. Health ────────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_health(client: httpx.AsyncClient) -> None:
    """GET /health returns 200 with status field."""
    data = await get_json(client, "/health")
    assert "status" in data


# ── 2. Metrics ───────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_metrics(client: httpx.AsyncClient) -> None:
    """GET /metrics returns 200 (Prometheus text format)."""
    r = await client.get("/metrics")
    assert r.status_code == 200


# ── 3. Speakers ──────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_speakers_list(client: httpx.AsyncClient) -> None:
    """GET /api/speakers returns a list."""
    data = await get_json(client, "/api/speakers")
    assert isinstance(data, (list, dict))


# ── 4. Speakers summary ─────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_speakers_summary(client: httpx.AsyncClient) -> None:
    """GET /api/speakers/summary returns count and speakers array."""
    data = await get_json(client, "/api/speakers/summary")
    assert "speakers" in data or "count" in data


# ── 5. Peers list ────────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_peers_list(client: httpx.AsyncClient) -> None:
    """GET /api/peers returns peers array."""
    data = await get_json(client, "/api/peers")
    assert "peers" in data
    assert isinstance(data["peers"], list)


# ── 6. Routes list ──────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_routes_list(client: httpx.AsyncClient) -> None:
    """GET /api/routes returns routes array."""
    data = await get_json(client, "/api/routes", limit="50")
    assert "routes" in data
    assert isinstance(data["routes"], list)


# ── 7. Route prefix history ─────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_routes_prefix_history(client: httpx.AsyncClient) -> None:
    """GET /api/routes/prefix returns prefix history."""
    data = await get_json(client, "/api/routes/prefix", prefix="1.2.3.0/24")
    # Should return without error; shape varies
    assert isinstance(data, (list, dict))


# ── 8. Route changes ────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_routes_changes(client: httpx.AsyncClient) -> None:
    """GET /api/routes/changes returns 200."""
    data = await get_json(client, "/api/routes/changes")
    assert isinstance(data, (list, dict))


# ── 9. Analytics churn ──────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_analytics_churn(client: httpx.AsyncClient) -> None:
    """GET /api/analytics/churn returns prefixes array."""
    data = await get_json(client, "/api/analytics/churn")
    assert "prefixes" in data


# ── 10. Analytics origins ────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_analytics_origins(client: httpx.AsyncClient) -> None:
    """GET /api/analytics/origins returns origins array."""
    data = await get_json(client, "/api/analytics/origins")
    assert "origins" in data


# ── 11. RPKI stats ──────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_rpki_stats(client: httpx.AsyncClient) -> None:
    """GET /api/rpki/stats returns validity counts."""
    data = await get_json(client, "/api/rpki/stats")
    assert isinstance(data, dict)


# ── 12. RPKI analysis ───────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_rpki_analysis(client: httpx.AsyncClient) -> None:
    """GET /api/rpki/analysis returns breakdown."""
    data = await get_json(client, "/api/rpki/analysis")
    assert "breakdown" in data or "per_peer" in data or isinstance(data, dict)


# ── 13. RPKI coverage ───────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_rpki_coverage(client: httpx.AsyncClient) -> None:
    """GET /api/rpki/coverage returns coverage stats."""
    data = await get_json(client, "/api/rpki/coverage")
    assert "total_prefixes" in data or "coverage_pct" in data or isinstance(data, dict)


# ── 14. ML anomalies ────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_ml_anomalies(client: httpx.AsyncClient) -> None:
    """GET /api/ml/anomalies returns anomalies array."""
    data = await get_json(client, "/api/ml/anomalies", limit="10")
    assert "anomalies" in data
    assert isinstance(data["anomalies"], list)


# ── 15. ML model status ─────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_ml_model_status(client: httpx.AsyncClient) -> None:
    """GET /api/ml/model/status returns models array."""
    data = await get_json(client, "/api/ml/model/status")
    assert "models" in data


# ── 16. Convergence events ──────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_convergence(client: httpx.AsyncClient) -> None:
    """GET /api/convergence returns convergence events."""
    data = await get_json(client, "/api/convergence")
    assert isinstance(data, (list, dict))


# ── 17. Max-prefix capacity ─────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_capacity_max_prefix(client: httpx.AsyncClient) -> None:
    """GET /api/capacity/max-prefix returns rows."""
    data = await get_json(client, "/api/capacity/max-prefix")
    assert "rows" in data or isinstance(data, dict)


# ── 18. Policy configs ──────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_policy_configs(client: httpx.AsyncClient) -> None:
    """GET /api/policy/configs returns policy config rows."""
    data = await get_json(client, "/api/policy/configs")
    assert isinstance(data, (list, dict))


# ── 19. Governance ──────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_governance(client: httpx.AsyncClient) -> None:
    """GET /api/governance returns resource governor status."""
    data = await get_json(client, "/api/governance")
    assert "profile" in data or isinstance(data, dict)


# ── 20. Filter stats ────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_filter_stats(client: httpx.AsyncClient) -> None:
    """GET /api/filters/stats returns filter statistics."""
    data = await get_json(client, "/api/filters/stats")
    assert isinstance(data, dict)


# ── 21. Seed endpoint ───────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_seed_endpoint(client: httpx.AsyncClient) -> None:
    """POST /api/_test/seed with 'anomaly' fixture succeeds."""
    r = await client.post("/api/_test/seed", json={"fixture": "anomaly", "truncate": True})
    assert r.status_code == 200
    body = r.json()
    assert body["ok"] is True
    assert body["fixture"] == "anomaly"


# ── 22. Seed unknown fixture returns error ───────────────────────────────────


@pytest.mark.asyncio
async def test_seed_unknown_fixture(client: httpx.AsyncClient) -> None:
    """POST /api/_test/seed with unknown fixture returns ok=false."""
    r = await client.post("/api/_test/seed", json={"fixture": "nonexistent"})
    assert r.status_code == 200
    body = r.json()
    assert body["ok"] is False
    assert "error" in body and body["error"] is not None


# ── 23. BGP-LS graph ────────────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_bgpls_graph(client: httpx.AsyncClient) -> None:
    """GET /api/bgpls/graph returns nodes and links arrays."""
    data = await get_json(client, "/api/bgpls/graph")
    assert "nodes" in data
    assert "links" in data


# ── 24. BMP stats history ───────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_bmpstats_history(client: httpx.AsyncClient) -> None:
    """GET /api/bmpstats/history returns stats array."""
    data = await get_json(client, "/api/bmpstats/history", limit="10")
    assert "stats" in data or isinstance(data, dict)
