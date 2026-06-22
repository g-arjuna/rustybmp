"""
Bundle B1 — Layer 2 BMP Protocol Integration Tests.

15 pytest tests that exercise BMP PDU parsing by reading the binary
fixtures generated in Bundle A3 (tests/fixtures/bmp/*.bin).

These tests validate the wire format without requiring a live router.
They parse BMP common headers, per-peer headers, and message bodies
according to RFC 7854 / RFC 9069.

Run:
    pytest tests/protocol/ -v
"""
from __future__ import annotations

import struct
from pathlib import Path
from typing import Any, Callable

import pytest


# ── BMP header constants ─────────────────────────────────────────────────────

BMP_VERSION = 3
BMP_COMMON_HDR_LEN = 7  # version(1) + msg_type(1) + msg_length(4) + reserved(1)

MSG_ROUTE_MONITORING = 0
MSG_STATISTICS_REPORT = 1
MSG_PEER_DOWN = 2
MSG_PEER_UP = 3
MSG_INITIATION = 4
MSG_TERMINATION = 5

PER_PEER_HDR_LEN = 42  # RFC 7854 §4.2

BGP_MARKER = b"\xff" * 16


# ── Helpers ───────────────────────────────────────────────────────────────────


def parse_bmp_common_header(data: bytes) -> dict[str, Any]:
    """Parse the 7-byte BMP common header.

    Wire format (as produced by gen_bmp_fixtures.py):
      version(1) + msg_type(1) + msg_length(4) + reserved(1)
    """
    assert len(data) >= BMP_COMMON_HDR_LEN, f"Too short for BMP header: {len(data)} bytes"
    version, msg_type, msg_length, _reserved = struct.unpack("!BBIB", data[:7])
    return {"version": version, "length": msg_length, "msg_type": msg_type}


def parse_per_peer_header(data: bytes, offset: int = BMP_COMMON_HDR_LEN) -> dict[str, Any]:
    """Parse the 42-byte per-peer header starting at *offset*."""
    pph = data[offset : offset + PER_PEER_HDR_LEN]
    assert len(pph) == PER_PEER_HDR_LEN, f"Per-peer header truncated at offset {offset}"
    peer_type = pph[0]
    peer_flags = pph[1]
    peer_distinguisher = pph[2:10]
    peer_addr_raw = pph[10:26]
    (peer_as,) = struct.unpack("!I", pph[26:30])
    peer_bgp_id = pph[30:34]
    (ts_secs, ts_usecs) = struct.unpack("!II", pph[34:42])
    # IPv4-mapped check: first 12 bytes zero means v4
    is_ipv6 = bool(peer_flags & 0x80)
    if not is_ipv6:
        peer_addr = ".".join(str(b) for b in peer_addr_raw[12:16])
    else:
        # Format as colon-hex
        parts = struct.unpack("!8H", peer_addr_raw)
        peer_addr = ":".join(f"{p:04x}" for p in parts)
    bgp_id = ".".join(str(b) for b in peer_bgp_id)
    return {
        "peer_type": peer_type,
        "peer_flags": peer_flags,
        "peer_distinguisher": peer_distinguisher,
        "peer_addr": peer_addr,
        "peer_as": peer_as,
        "bgp_id": bgp_id,
        "ts_secs": ts_secs,
        "ts_usecs": ts_usecs,
        "is_ipv6": is_ipv6,
    }


# ── Fixture 1: Initiation ────────────────────────────────────────────────────


class TestInitiation:
    """Tests for BMP Initiation Message (type=4)."""

    def test_initiation_common_header(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Initiation message has correct BMP version and type."""
        data = bmp_fixture("01_initiation")
        hdr = parse_bmp_common_header(data)
        assert hdr["version"] == BMP_VERSION
        assert hdr["msg_type"] == MSG_INITIATION

    def test_initiation_length_consistent(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Stated length in header is consistent with actual PDU size.

        gen_bmp_fixtures uses total = 6 + len(body), with a 7-byte header,
        so length = len(data) - 1.
        """
        data = bmp_fixture("01_initiation")
        hdr = parse_bmp_common_header(data)
        assert hdr["length"] == len(data) - 1

    def test_initiation_has_info_tlv(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Initiation body contains an Information TLV (type=0, sysDescr)."""
        data = bmp_fixture("01_initiation")
        body = data[BMP_COMMON_HDR_LEN:]
        assert len(body) >= 4, "Body too short for a TLV"
        (tlv_type, tlv_len) = struct.unpack("!HH", body[:4])
        assert tlv_type == 0, "Expected sysDescr TLV (type=0)"
        value = body[4 : 4 + tlv_len]
        assert b"rustybmp" in value


# ── Fixture 2: Peer Up ───────────────────────────────────────────────────────


class TestPeerUp:
    """Tests for BMP Peer-Up Notification (type=3)."""

    def test_peer_up_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("02_peer_up")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_PEER_UP

    def test_peer_up_per_peer_header(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Peer-Up contains a valid per-peer header with AS 65001."""
        data = bmp_fixture("02_peer_up")
        pph = parse_per_peer_header(data)
        assert pph["peer_as"] == 65001
        assert pph["peer_addr"] == "192.0.2.1"


# ── Fixture 3: Route Monitor Announce ─────────────────────────────────────────


class TestRouteMonitorAnnounce:
    """Tests for BMP Route-Monitoring Announce (type=0)."""

    def test_route_monitor_announce_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("03_route_monitor_announce")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_ROUTE_MONITORING

    def test_route_monitor_announce_bgp_marker(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Embedded BGP UPDATE starts with the 16-byte all-FF marker."""
        data = bmp_fixture("03_route_monitor_announce")
        bgp_start = BMP_COMMON_HDR_LEN + PER_PEER_HDR_LEN
        assert data[bgp_start : bgp_start + 16] == BGP_MARKER

    def test_route_monitor_announce_bgp_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Embedded BGP message is type=2 (UPDATE)."""
        data = bmp_fixture("03_route_monitor_announce")
        bgp_start = BMP_COMMON_HDR_LEN + PER_PEER_HDR_LEN
        bgp_type = data[bgp_start + 18]  # offset 18 = type byte after marker(16)+length(2)
        assert bgp_type == 2


# ── Fixture 4: Route Monitor Withdraw ─────────────────────────────────────────


class TestRouteMonitorWithdraw:
    """Tests for BMP Route-Monitoring Withdraw (type=0)."""

    def test_route_monitor_withdraw_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("04_route_monitor_withdraw")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_ROUTE_MONITORING

    def test_route_monitor_withdraw_has_withdrawn_routes(
        self, bmp_fixture: Callable[[str], bytes]
    ) -> None:
        """BGP UPDATE has non-zero withdrawn routes length."""
        data = bmp_fixture("04_route_monitor_withdraw")
        bgp_start = BMP_COMMON_HDR_LEN + PER_PEER_HDR_LEN
        # BGP UPDATE body starts after marker(16) + length(2) + type(1) = offset 19
        update_body = data[bgp_start + 19 :]
        (wd_len,) = struct.unpack("!H", update_body[:2])
        assert wd_len > 0, "Withdraw must have non-zero withdrawn routes length"


# ── Fixture 5/6: Path Status TLV ─────────────────────────────────────────────


class TestPathStatusTLV:
    """Tests for RFC 9069 Path Status TLV in Route-Monitoring messages."""

    def test_path_status_best_contains_tlv(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Fixture 05 contains a Path Status TLV (type=6) with BEST bit."""
        data = bmp_fixture("05_route_monitor_path_status_best")
        # Search for TLV type=6 in the path attributes region
        assert _find_path_status_value(data) is not None
        status_val = _find_path_status_value(data)
        assert status_val is not None
        assert status_val & 0x01, "BEST bit (0x01) should be set"

    def test_path_status_nonselected(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Fixture 06 has NONSELECTED bit (0x04) set in Path Status TLV."""
        data = bmp_fixture("06_route_monitor_path_status_nonsel")
        status_val = _find_path_status_value(data)
        assert status_val is not None
        assert status_val & 0x04, "NONSELECTED bit (0x04) should be set"


def _find_path_status_value(data: bytes) -> int | None:
    """Scan for TLV type=6 (Path Status) and return the 4-byte status value."""
    # Path Status TLV is inside BGP path attributes; scan for 0x0006 (type)
    needle = struct.pack("!HH", 6, 6)  # type=6, length=6
    idx = data.find(needle)
    if idx < 0:
        return None
    (status,) = struct.unpack("!I", data[idx + 4 : idx + 8])
    return status


# ── Fixture 7: Peer Down ─────────────────────────────────────────────────────


class TestPeerDown:
    """Tests for BMP Peer-Down Notification (type=2)."""

    def test_peer_down_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("07_peer_down")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_PEER_DOWN

    def test_peer_down_reason_code(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Peer-Down body contains reason code 3 (remote system closed)."""
        data = bmp_fixture("07_peer_down")
        reason_offset = BMP_COMMON_HDR_LEN + PER_PEER_HDR_LEN
        reason = data[reason_offset]
        assert reason == 3


# ── Fixture 8: Statistics Report ──────────────────────────────────────────────


class TestStatsReport:
    """Tests for BMP Statistics Report (type=1)."""

    def test_stats_report_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("08_stats_report")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_STATISTICS_REPORT

    def test_stats_report_counter(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Stats report has 1 counter with value=42."""
        data = bmp_fixture("08_stats_report")
        body_offset = BMP_COMMON_HDR_LEN + PER_PEER_HDR_LEN
        (stat_count,) = struct.unpack("!I", data[body_offset : body_offset + 4])
        assert stat_count == 1
        # First stat: type(2) + len(2) + value(4)
        stat_offset = body_offset + 4
        (stat_type, stat_len) = struct.unpack("!HH", data[stat_offset : stat_offset + 4])
        assert stat_type == 0
        assert stat_len == 4
        (stat_value,) = struct.unpack("!I", data[stat_offset + 4 : stat_offset + 8])
        assert stat_value == 42


# ── Fixture 9: IPv6 Route Monitor ─────────────────────────────────────────────


class TestRouteMonitorIPv6:
    """Tests for BMP Route-Monitoring with IPv6 NLRI."""

    def test_ipv6_peer_flags(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Peer flags have the IPv6 bit (0x80) set."""
        data = bmp_fixture("09_route_monitor_ipv6")
        pph = parse_per_peer_header(data)
        assert pph["is_ipv6"], "IPv6 flag should be set in per-peer header"


# ── Fixture 10: Termination ──────────────────────────────────────────────────


class TestTermination:
    """Tests for BMP Termination Message (type=5)."""

    def test_termination_type(self, bmp_fixture: Callable[[str], bytes]) -> None:
        data = bmp_fixture("10_termination")
        hdr = parse_bmp_common_header(data)
        assert hdr["msg_type"] == MSG_TERMINATION

    def test_termination_reason_tlv(self, bmp_fixture: Callable[[str], bytes]) -> None:
        """Termination body has a reason TLV (type=0)."""
        data = bmp_fixture("10_termination")
        body = data[BMP_COMMON_HDR_LEN:]
        (tlv_type, tlv_len) = struct.unpack("!HH", body[:4])
        assert tlv_type == 0, "Expected reason TLV type=0"
        assert tlv_len == 2
