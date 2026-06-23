"""
Layer 5 — XRd RFC 9972 Stats Test (Tier 1)

Requires: cargo build -p rbmp-server --bins, containerlab, docker,
ios-xr/xrd-control-plane:24.4.2 (licensed)
Run: pytest tests/scenarios/02_xrd_rfc9972/ -v

The test starts rustybmp as a host process, spins up the ContainerLab XRd
topology, waits for BMP sessions and RFC 9972 stats to arrive, exercises the
HTTP API, then tears everything down.
"""
import os
import shutil
import subprocess
import time
from pathlib import Path

import pytest
import requests

SCENARIO_DIR = os.path.dirname(__file__)
CLAB_FILE = os.path.join(SCENARIO_DIR, "topology.clab.yml")
CONFIG_FILE = os.path.join(SCENARIO_DIR, "configs", "rustybmp.toml")
SERVER_BIN = Path("target/debug/rustybmp")
API_BASE = os.getenv("RUSTYBMP_API_BASE", "http://127.0.0.1:17878")
TIMEOUT = 240


def _stop_process(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


@pytest.fixture(scope="module")
def rustybmp_server():
    """Start rustybmp on the host so XRd nodes can target the host listener."""
    if not SERVER_BIN.exists():
        pytest.fail(
            f"Missing {SERVER_BIN}; run `cargo build -p rbmp-server --bins` first"
        )

    log_dir = Path("runtime/test_results")
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "layer5_rustybmp.log"
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
    try:
        subprocess.run(
            ["containerlab", "deploy", "-t", CLAB_FILE, "--reconfigure"],
            check=True,
            timeout=180,
            capture_output=True,
            text=True,
        )
    except subprocess.CalledProcessError as exc:
        stderr = (exc.stderr or "").lower()
        if (
            "pull access denied" in stderr
            or "no such image" in stderr
            or "requested access to the resource is denied" in stderr
        ):
            pytest.skip(
                "XRd image unavailable; install or log in for "
                "`ios-xr/xrd-control-plane:24.4.2` to run Layer 5"
            )
        raise
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


@pytest.fixture(scope="module")
def routes_ready(bmp_sessions_up):
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
        time.sleep(5)
    pytest.fail(f"Expected XRd route announcements within {TIMEOUT}s")


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

    def test_stats_have_type_30(self, bmp_sessions_up):
        """RFC 9972 type 30 = per-AFI/SAFI pre-route-limit Adj-RIB-In headroom."""
        r = requests.get(
            f"{API_BASE}/api/bmpstats/history",
            params={"limit": "200"},
            timeout=10,
        )
        stats = r.json().get("stats", [])
        stat_types = [s.get("stat_type") for s in stats]
        assert 30 in stat_types, f"Stats type 30 not found in: {set(stat_types)}"

    def test_stats_include_afi_safi_breakdown(self, bmp_sessions_up):
        """RFC 9972 per-AFI/SAFI gauges should preserve AFI/SAFI in the API."""
        r = requests.get(
            f"{API_BASE}/api/bmpstats/history",
            params={"limit": "200"},
            timeout=10,
        )
        stats = r.json().get("stats", [])
        with_afi = [s for s in stats if s.get("afi") is not None and s.get("safi") is not None]
        assert with_afi, "Expected at least one stats record with AFI/SAFI fields"

    def test_xrd_prefixes_in_route_table(self, routes_ready):
        assert len(routes_ready) >= 1, "Expected routes from XRd after BMP init"

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
