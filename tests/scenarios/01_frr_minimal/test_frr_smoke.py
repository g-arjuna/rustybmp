"""
Layer 4 — FRR Minimal Smoke Test (Tier 0)

Requires: cargo build -p rbmp-server --bins, containerlab, docker,
quay.io/frrouting/frr:10.6.1
Run: pytest tests/scenarios/01_frr_minimal/ -v --timeout=180

The test starts rustybmp as a host process, spins up the ContainerLab FRR
topology, waits for BMP sessions to establish, exercises the HTTP API, then
tears everything down.
"""
import os
import shutil
import subprocess
import time
from pathlib import Path

import requests
import pytest

SCENARIO_DIR = os.path.dirname(__file__)
CLAB_FILE = os.path.join(os.path.dirname(__file__), "topology.clab.yml")
CONFIG_FILE = os.path.join(SCENARIO_DIR, "configs", "rustybmp.toml")
SERVER_BIN = Path("target/debug/rustybmp")
API_BASE = os.getenv("RUSTYBMP_API_BASE", "http://127.0.0.1:17878")
TIMEOUT = 120  # seconds to wait for BMP sessions


def _stop_process(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


# ── Fixtures ─────────────────────────────────────────────────────────────────

@pytest.fixture(scope="module")
def rustybmp_server():
    """Start rustybmp on the host so lab nodes can target the host listener."""
    if not SERVER_BIN.exists():
        pytest.fail(
            f"Missing {SERVER_BIN}; run `cargo build -p rbmp-server --bins` first"
        )

    log_dir = Path("runtime/test_results")
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "layer4_rustybmp.log"
    with log_path.open("wb") as log_file:
        proc = subprocess.Popen(
            [str(SERVER_BIN), CONFIG_FILE],
            stdout=log_file,
            stderr=subprocess.STDOUT,
            start_new_session=True,
        )

        deadline = time.time() + 20
        while time.time() < deadline:
            if proc.poll() is not None:
                pytest.fail(
                    f"rustybmp exited early; see {log_path} for captured output"
                )
            try:
                r = requests.get(f"{API_BASE}/health", timeout=1)
                if r.status_code == 200 and r.json().get("status") == "ok":
                    yield proc
                    break
            except Exception:
                pass
            time.sleep(1)
        else:
            _stop_process(proc)
            pytest.fail(
                f"rustybmp did not become healthy within 20s; see {log_path}"
            )

        _stop_process(proc)


@pytest.fixture(scope="module")
def clab_topology(rustybmp_server):
    """Bring up the FRR-only ContainerLab topology, yield, then tear down."""
    subprocess.run(
        ["containerlab", "deploy", "-t", CLAB_FILE, "--reconfigure"],
        check=True,
        timeout=120,
    )
    yield
    subprocess.run(
        ["containerlab", "destroy", "-t", CLAB_FILE, "--cleanup"],
        check=True,
        timeout=60,
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


@pytest.fixture(scope="module")
def announced_routes(peers_up):
    """Wait until at least one announced route is visible via the API."""
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(
                f"{API_BASE}/api/routes",
                params={"limit": "20", "action": "announce"},
                timeout=10,
            )
            routes = r.json().get("routes", [])
            if routes:
                return routes
        except Exception:
            pass
        time.sleep(3)
    pytest.fail(f"Expected announced routes within {TIMEOUT}s")


def wait_for_prefix(prefix: str) -> list[dict]:
    """Poll /api/routes until a specific prefix is present or timeout expires."""
    deadline = time.time() + TIMEOUT
    while time.time() < deadline:
        try:
            r = requests.get(
                f"{API_BASE}/api/routes",
                params={"prefix": prefix, "limit": "10", "action": "announce"},
                timeout=10,
            )
            routes = r.json().get("routes", [])
            if any(rt.get("prefix") == prefix for rt in routes):
                return routes
        except Exception:
            pass
        time.sleep(3)
    pytest.fail(f"Expected prefix {prefix} within {TIMEOUT}s")


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

    def test_routes_received(self, announced_routes):
        """After BMP init, route_events table must have announcements."""
        assert len(announced_routes) >= 1, "Expected ≥1 announced route after BMP init"

    def test_known_prefix_present(self, announced_routes):
        """frr1 announces 10.100.1.0/24 — must appear in route table."""
        routes = wait_for_prefix("10.100.1.0/24")
        assert any(rt["prefix"] == "10.100.1.0/24" for rt in routes), (
            "Expected 10.100.1.0/24 from frr1 in route table"
        )

    def test_frr2_prefix_present(self, announced_routes):
        """frr2 announces 10.200.1.0/24."""
        routes = wait_for_prefix("10.200.1.0/24")
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
