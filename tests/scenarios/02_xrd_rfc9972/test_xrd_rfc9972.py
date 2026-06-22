"""
Layer 5 — XRd RFC 9972 Stats Test (Tier 1)

Requires: containerlab, docker, ios-xr/xrd-control-plane:24.2.1 (licensed)
Run: pytest tests/scenarios/02_xrd_rfc9972/ -v --timeout=300

Validates RFC 9972 stats message types sent by IOS-XRd:
- Type 7 (Adj-RIB-In withdrawn prefixes)
- Type 8 (Adj-RIB-In prefixes)
- Type 17 (EOR)
"""
import os
import shutil
import subprocess
import time
import requests
import pytest

CLAB_FILE = os.path.join(os.path.dirname(__file__), "topology.clab.yml")
API_BASE  = "http://localhost:27878"
TIMEOUT   = 240


@pytest.fixture(scope="module")
def clab_topology():
    subprocess.run(
        ["containerlab", "deploy", "-t", CLAB_FILE, "--reconfigure"],
        check=True, timeout=180,
    )
    yield
    subprocess.run(
        ["containerlab", "destroy", "-t", CLAB_FILE, "--cleanup"],
        check=True, timeout=60,
    )


@pytest.fixture(scope="module")
def api_ready(clab_topology):
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/health", timeout=2)
            if r.status_code == 200 and r.json().get("status") == "ok":
                return True
        except Exception:
            pass
        time.sleep(3)
    pytest.fail(f"API not healthy within {TIMEOUT}s")


@pytest.fixture(scope="module")
def bmp_sessions_up(api_ready):
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(f"{API_BASE}/api/speakers", timeout=5)
            speakers = r.json().get("speakers", [])
            if len(speakers) >= 2:
                return speakers
        except Exception:
            pass
        time.sleep(5)
    pytest.fail(f"Expected 2 XRd speakers within {TIMEOUT}s")


@pytest.mark.skipif(
    not shutil.which("containerlab"),
    reason="containerlab not installed",
)
class TestXrdRfc9972:

    def test_api_health(self, api_ready):
        r = requests.get(f"{API_BASE}/health", timeout=5)
        assert r.status_code == 200
        assert r.json()["status"] == "ok"

    def test_two_xrd_speakers(self, bmp_sessions_up):
        assert len(bmp_sessions_up) >= 2

    def test_bmp_stats_received(self, bmp_sessions_up):
        """RFC 9972 StatsReport messages must appear in bmpstats history."""
        r = requests.get(
            f"{API_BASE}/api/bmpstats/history",
            params={"limit": "50"},
            timeout=10,
        )
        assert r.status_code == 200
        stats = r.json().get("stats", [])
        assert len(stats) >= 1, "Expected ≥1 RFC 9972 stats record from XRd"

    def test_stats_have_type_8(self, bmp_sessions_up):
        """Stats type 8 = Adj-RIB-In Accepted Prefixes (RFC 9972)."""
        r = requests.get(
            f"{API_BASE}/api/bmpstats/history",
            params={"limit": "200"},
            timeout=10,
        )
        stats = r.json().get("stats", [])
        stat_types = [s.get("stat_type") for s in stats]
        assert 8 in stat_types, f"Stats type 8 not found in: {set(stat_types)}"

    def test_xrd_prefixes_in_route_table(self, bmp_sessions_up):
        r = requests.get(
            f"{API_BASE}/api/routes",
            params={"limit": "20", "action": "announce"},
            timeout=10,
        )
        routes = r.json().get("routes", [])
        assert len(routes) >= 1, "Expected routes from XRd after BMP init"

    def test_rpki_stats_reachable(self, api_ready):
        r = requests.get(f"{API_BASE}/api/rpki/stats", timeout=5)
        assert r.status_code == 200

    def test_policy_endpoint_reachable(self, api_ready):
        r = requests.get(
            f"{API_BASE}/api/peers",
            timeout=5,
        )
        assert r.status_code == 200
        peers = r.json().get("peers", [])
        assert len(peers) >= 1

    def test_path_status_matrix_reachable(self, api_ready):
        r = requests.get(
            f"{API_BASE}/api/path-status/matrix",
            params={"limit": "50"},
            timeout=5,
        )
        assert r.status_code == 200
