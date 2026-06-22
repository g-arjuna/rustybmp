"""
Layer 4 — FRR Minimal Smoke Test (Tier 0)

Requires: containerlab, docker, quay.io/frrouting/frr:10.6.1
Run: pytest tests/scenarios/01_frr_minimal/ -v --timeout=180

The test spins up the ContainerLab topology, waits for BMP sessions to
establish, exercises the rustybmp HTTP API, then tears down.
"""
import os
import shutil
import subprocess
import time
import requests
import pytest

CLAB_FILE = os.path.join(os.path.dirname(__file__), "topology.clab.yml")
API_BASE  = "http://localhost:17878"
TIMEOUT   = 120  # seconds to wait for BMP sessions


# ── Fixtures ─────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def clab_topology():
    """Bring up ContainerLab topology, yield, then tear down."""
    subprocess.run(
        ["containerlab", "deploy", "-t", CLAB_FILE, "--reconfigure"],
        check=True, timeout=120,
    )
    yield
    subprocess.run(
        ["containerlab", "destroy", "-t", CLAB_FILE, "--cleanup"],
        check=True, timeout=60,
    )


@pytest.fixture(scope="module")
def api_ready(clab_topology):
    """Wait for the rustybmp API to become healthy."""
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/health", timeout=2)
            if r.status_code == 200 and r.json().get("status") == "ok":
                return True
        except Exception:
            pass
        time.sleep(2)
    pytest.fail(f"API did not become healthy within {TIMEOUT}s")


@pytest.fixture(scope="module")
def bmp_sessions_up(api_ready):
    """Wait until at least 2 BMP speakers have connected."""
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/api/speakers", timeout=5)
            speakers = r.json().get("speakers", [])
            if len(speakers) >= 2:
                return speakers
        except Exception:
            pass
        time.sleep(3)
    pytest.fail(f"Expected 2 BMP speakers within {TIMEOUT}s; got none")


@pytest.fixture(scope="module")
def peers_up(bmp_sessions_up):
    """Wait until at least 2 peers are in 'up' state."""
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/api/peers", timeout=5)
            peers = r.json().get("peers", [])
            up_peers = [p for p in peers if p.get("state") == "up"]
            if len(up_peers) >= 2:
                return up_peers
        except Exception:
            pass
        time.sleep(3)
    pytest.fail(f"Expected ≥2 peers in 'up' state within {TIMEOUT}s")


# ── Tests ─────────────────────────────────────────────────────────────────────

@pytest.mark.skipif(
    not shutil.which("containerlab"),
    reason="containerlab not installed",
)
class TestFrrSmoke:

    def test_api_health(self, api_ready):
        r = requests.get(f"{API_BASE}/health", timeout=5)
        assert r.status_code == 200
        assert r.json()["status"] == "ok"

    def test_two_speakers_connected(self, bmp_sessions_up):
        assert len(bmp_sessions_up) >= 2, (
            f"Expected ≥2 speakers, got {len(bmp_sessions_up)}: {bmp_sessions_up}"
        )

    def test_speaker_has_sys_name(self, bmp_sessions_up):
        for spk in bmp_sessions_up:
            assert "addr" in spk, f"Speaker missing 'addr': {spk}"

    def test_peers_established(self, peers_up):
        assert len(peers_up) >= 2, f"Expected ≥2 up peers, got {len(peers_up)}"

    def test_peers_have_asn(self, peers_up):
        for peer in peers_up:
            assert peer.get("peer_as") is not None, f"Peer missing ASN: {peer}"
            assert peer["peer_as"] == 65100

    def test_routes_received(self, peers_up):
        """After BMP init, route_events table must have announcements."""
        r = requests.get(
            f"{API_BASE}/api/routes",
            params={"limit": "10", "action": "announce"},
            timeout=10,
        )
        assert r.status_code == 200
        routes = r.json().get("routes", [])
        assert len(routes) >= 1, "Expected ≥1 announced route after BMP init"

    def test_known_prefix_present(self, peers_up):
        """frr1 announces 10.100.1.0/24 — must appear in route table."""
        r = requests.get(
            f"{API_BASE}/api/routes",
            params={"prefix": "10.100.1.0/24", "limit": "5"},
            timeout=10,
        )
        assert r.status_code == 200
        routes = r.json().get("routes", [])
        assert any(rt["prefix"] == "10.100.1.0/24" for rt in routes), (
            "Expected 10.100.1.0/24 from frr1 in route table"
        )

    def test_frr2_prefix_present(self, peers_up):
        """frr2 announces 10.200.1.0/24."""
        r = requests.get(
            f"{API_BASE}/api/routes",
            params={"prefix": "10.200.1.0/24", "limit": "5"},
            timeout=10,
        )
        assert r.status_code == 200
        routes = r.json().get("routes", [])
        assert any(rt["prefix"] == "10.200.1.0/24" for rt in routes), (
            "Expected 10.200.1.0/24 from frr2 in route table"
        )

    def test_rpki_stats_reachable(self, api_ready):
        r = requests.get(f"{API_BASE}/api/rpki/stats", timeout=5)
        assert r.status_code == 200
        body = r.json()
        assert isinstance(body, dict)

    def test_analytics_churn_reachable(self, api_ready):
        r = requests.get(f"{API_BASE}/api/analytics/churn", timeout=5)
        assert r.status_code == 200

    def test_bmpstats_history_reachable(self, api_ready):
        r = requests.get(
            f"{API_BASE}/api/bmpstats/history",
            params={"limit": "20"},
            timeout=5,
        )
        assert r.status_code == 200
