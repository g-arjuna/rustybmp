"""
Layer 5 — Cross-Vendor FRR ↔ XRd eBGP Smoke Test

Requires: cargo build -p rbmp-server --bins, containerlab, docker,
quay.io/frrouting/frr:10.6.1, ios-xr/xrd-control-plane:24.4.2 (licensed)

The test starts rustybmp as a host process, deploys a direct FRR↔XRd
ContainerLab topology, waits for the cross-vendor IPv4 unicast, IPv4
multicast, and IPv6 unicast eBGP sessions to converge, then verifies BMP/API
visibility for both vendors through the shared host collector.
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
TIMEOUT = 300


def _stop_process(proc: subprocess.Popen) -> None:
    if proc.poll() is not None:
        return
    proc.terminate()
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


def _wait_for_json(path: str, key: str, predicate, timeout: int = TIMEOUT):
    deadline = time.time() + timeout
    last_value = None
    while time.time() < deadline:
        try:
            response = requests.get(f"{API_BASE}{path}", timeout=10)
            response.raise_for_status()
            last_value = response.json().get(key)
            if predicate(last_value):
                return last_value
        except Exception:
            pass
        time.sleep(5)
    pytest.fail(f"Timed out waiting for {path}; last {key}={last_value!r}")


def _routes_for_prefix(prefix: str) -> list[dict]:
    response = requests.get(
        f"{API_BASE}/api/routes",
        params={"prefix": prefix, "limit": "20", "action": "announce"},
        timeout=10,
    )
    response.raise_for_status()
    return response.json().get("routes", [])


def _wait_for_prefix(prefix: str, timeout: int = TIMEOUT) -> list[dict]:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            routes = _routes_for_prefix(prefix)
            if any(route.get("prefix") == prefix for route in routes):
                return routes
        except Exception:
            pass
        time.sleep(5)
    pytest.fail(f"Timed out waiting for announced prefix {prefix}")


@pytest.fixture(scope="module")
def rustybmp_server():
    if not SERVER_BIN.exists():
        pytest.fail(
            f"Missing {SERVER_BIN}; run `cargo build -p rbmp-server --bins` first"
        )

    log_dir = Path("runtime/test_results")
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / "layer5_cross_vendor_rustybmp.log"
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
                response = requests.get(f"{API_BASE}/health", timeout=1)
                if (
                    response.status_code == 200
                    and response.json().get("status") == "ok"
                ):
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
            timeout=240,
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
                "Cross-vendor lab images unavailable; ensure FRR 10.6.1 and "
                "XRd 24.4.2 are installed locally before running this scenario"
            )
        raise
    yield
    subprocess.run(
        ["containerlab", "destroy", "-t", CLAB_FILE, "--cleanup"],
        check=True,
        timeout=90,
    )


@pytest.fixture(scope="module")
def api_ready(clab_topology):
    _wait_for_json("/health", "status", lambda status: status == "ok")
    return True


@pytest.fixture(scope="module")
def speakers_ready(api_ready):
    return _wait_for_json(
        "/api/speakers",
        "speakers",
        lambda speakers: isinstance(speakers, list) and len(speakers) >= 2,
    )


@pytest.fixture(scope="module")
def peers_ready(speakers_ready):
    return _wait_for_json(
        "/api/peers",
        "peers",
        lambda peers: (
            isinstance(peers, list)
            and len([peer for peer in peers if peer.get("state") == "up"]) >= 2
        ),
    )


@pytest.fixture(scope="module")
def stats_ready(speakers_ready):
    return _wait_for_json(
        "/api/bmpstats/history?limit=200",
        "stats",
        lambda stats: isinstance(stats, list) and len(stats) >= 1,
    )


@pytest.mark.skipif(
    not shutil.which("containerlab"),
    reason="containerlab not installed",
)
class TestCrossVendorFrrXrd:

    def test_api_health(self, api_ready):
        response = requests.get(f"{API_BASE}/health", timeout=5)
        assert response.status_code == 200
        assert response.json()["status"] == "ok"

    def test_two_speakers_connected(self, speakers_ready):
        assert len(speakers_ready) >= 2

    def test_speakers_include_both_vendors(self, speakers_ready):
        speaker_addrs = {speaker.get("addr") for speaker in speakers_ready}
        expected = {"172.20.23.2", "172.20.23.3"}
        assert expected.issubset(speaker_addrs), (
            f"Expected FRR and XRd speaker addresses {expected}, got {speaker_addrs}"
        )

    def test_cross_vendor_peers_up(self, peers_ready):
        up_peers = [peer for peer in peers_ready if peer.get("state") == "up"]
        assert len(up_peers) >= 2

    def test_cross_vendor_peer_asns_visible(self, peers_ready):
        peer_asns = {peer.get("peer_as") for peer in peers_ready}
        assert {65100, 65200}.issubset(peer_asns), (
            f"Expected cross-vendor peer ASNs 65100/65200, got {peer_asns}"
        )

    def test_frr_prefix_visible(self, peers_ready):
        routes = _wait_for_prefix("10.140.1.0/24")
        assert any(route.get("prefix") == "10.140.1.0/24" for route in routes)

    def test_xrd_prefix_visible(self, peers_ready):
        routes = _wait_for_prefix("10.240.1.0/24")
        assert any(route.get("prefix") == "10.240.1.0/24" for route in routes)

    def test_routes_cover_both_vendors(self, peers_ready):
        expected = [
            "10.140.1.0/24",
            "10.140.2.0/24",
            "10.240.1.0/24",
            "10.240.2.0/24",
        ]
        for prefix in expected:
            routes = _wait_for_prefix(prefix)
            assert any(route.get("prefix") == prefix for route in routes), (
                f"Expected cross-vendor prefix {prefix} to be queryable"
            )

    def test_xrd_ipv4_multicast_prefix_visible(self, peers_ready):
        routes = _wait_for_prefix("10.241.1.0/24")
        assert any(route.get("prefix") == "10.241.1.0/24" for route in routes)

    def test_ipv4_multicast_routes_visible_from_xrd(self, peers_ready):
        expected = [
            "10.241.1.0/24",
            "10.241.2.0/24",
        ]
        for prefix in expected:
            routes = _wait_for_prefix(prefix)
            assert any(route.get("prefix") == prefix for route in routes), (
                f"Expected XRd-originated IPv4 multicast prefix {prefix} to be queryable"
            )

    def test_frr_ipv6_prefix_visible(self, peers_ready):
        routes = _wait_for_prefix("2001:db8:140:1::/64")
        assert any(
            route.get("prefix") == "2001:db8:140:1::/64" for route in routes
        )

    def test_xrd_ipv6_prefix_visible(self, peers_ready):
        routes = _wait_for_prefix("2001:db8:240:1::/64")
        assert any(
            route.get("prefix") == "2001:db8:240:1::/64" for route in routes
        )

    def test_ipv6_routes_cover_both_vendors(self, peers_ready):
        expected = [
            "2001:db8:140:1::/64",
            "2001:db8:140:2::/64",
            "2001:db8:240:1::/64",
            "2001:db8:240:2::/64",
        ]
        for prefix in expected:
            routes = _wait_for_prefix(prefix)
            assert any(route.get("prefix") == prefix for route in routes), (
                f"Expected cross-vendor IPv6 prefix {prefix} to be queryable"
            )

    def test_xrd_stats_visible(self, stats_ready):
        stat_types = {stat.get("stat_type") for stat in stats_ready}
        assert {7, 8, 9, 10}.intersection(stat_types), (
            f"Expected XRd legacy/global stats counters, got {stat_types}"
        )

    def test_policy_and_path_status_endpoints_reachable(self, api_ready):
        peers_response = requests.get(f"{API_BASE}/api/peers", timeout=5)
        matrix_response = requests.get(
            f"{API_BASE}/api/path-status/matrix",
            params={"limit": "50"},
            timeout=5,
        )
        assert peers_response.status_code == 200
        assert matrix_response.status_code == 200
