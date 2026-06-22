#!/usr/bin/env python3
"""
Bundle A3 — BMP fixture generator.

Generates minimal but RFC-correct BMP PDU binary captures in
tests/fixtures/bmp/*.bin  These are used by integration and unit tests
as deterministic parse targets (no live router required).

Each fixture is constructed byte-by-byte from the BMP RFC 7854 /
RFC 9069 (Path Status TLV) specifications.

Run from the repo root:
    python3 scripts/gen_bmp_fixtures.py
"""
import os
import struct

FIXTURES_DIR = os.path.join(os.path.dirname(__file__), "..", "tests", "fixtures", "bmp")


def write(name: str, data: bytes) -> None:
    os.makedirs(FIXTURES_DIR, exist_ok=True)
    path = os.path.join(FIXTURES_DIR, name)
    with open(path, "wb") as f:
        f.write(data)
    print(f"  wrote {name:40s}  ({len(data):4d} bytes)")


# ── Helpers ───────────────────────────────────────────────────────────────────

def bmp_header(msg_type: int, body: bytes) -> bytes:
    """Build a BMP common header + body.  Version=3, length=6+len(body)."""
    total = 6 + len(body)
    return struct.pack("!BBIB", 3, msg_type, total, 0) + body


def peer_header(peer_as: int = 65001,
                peer_addr_v4: bytes = b"\xc0\x00\x02\x01",
                peer_bgp_id: bytes = b"\xc0\x00\x02\x01",
                ts_secs: int = 1_700_000_000,
                ts_usecs: int = 0,
                peer_flags: int = 0x00) -> bytes:
    """
    BMP Per-Peer Header (RFC 7854 §4.2).
    peer_type=0 (global), peer_flags, peer_distinguisher (8 bytes zeros),
    peer_addr (16 bytes, v4 mapped), peer_as (4 bytes), peer_bgp_id (4 bytes),
    timestamp_sec (4 bytes), timestamp_usec (4 bytes).
    """
    peer_type = 0
    peer_distinguisher = b"\x00" * 8
    peer_addr = b"\x00" * 12 + peer_addr_v4   # v4-in-v6, zero-pad 12 bytes
    return (
        struct.pack("!BB", peer_type, peer_flags) +
        peer_distinguisher +
        peer_addr +
        struct.pack("!II", peer_as, int.from_bytes(peer_bgp_id, "big")) +
        struct.pack("!II", ts_secs, ts_usecs)
    )


def bgp_update_body(nlri_prefix: bytes = b"\x18\xc0\x00\x02",
                    withdrawn: bytes = b"") -> bytes:
    """Minimal BGP UPDATE: withdrawn_routes + path_attrs + NLRI."""
    # Empty path attributes (keep it minimal; parsers must handle no attrs)
    withdrawn_len = struct.pack("!H", len(withdrawn))
    path_attrs = b""
    path_attrs_len = struct.pack("!H", len(path_attrs))
    return (
        struct.pack("!H", 19) +      # marker (just length placeholder here)
        struct.pack("!BB", 2, 0) +   # BGP type=UPDATE, stub
        withdrawn_len + withdrawn +
        path_attrs_len + path_attrs +
        nlri_prefix
    )


# ── Fixture 1: Initiation message (type=4) ───────────────────────────────────
def make_initiation() -> bytes:
    # Information TLV: type=0 (sysDescr), value="rustybmp-test-router"
    value = b"rustybmp-test-router"
    tlv = struct.pack("!HH", 0, len(value)) + value
    body = tlv
    return bmp_header(4, body)


# ── Fixture 2: Peer-Up Notification (type=3) ─────────────────────────────────
def make_peer_up() -> bytes:
    ph = peer_header()
    local_addr = b"\x00" * 12 + b"\xc0\x00\x02\x02"  # 192.0.2.2 v4-mapped
    local_port = struct.pack("!H", 179)
    remote_port = struct.pack("!H", 50000)
    # Minimal OPEN: version=4, my_as=65001, hold_time=90, bgp_id=192.0.2.1
    bgp_open_len = 19 + 10  # BGP header(19) + OPEN fixed(10)
    open_msg = struct.pack("!BBHB",
                           4, 1, bgp_open_len, 4)   # marker-placeholder(ver), type=OPEN, len, BGP ver
    open_msg += struct.pack("!HH", 65001, 90)        # My AS, Hold Time
    open_msg += b"\xc0\x00\x02\x01" + b"\x00"       # BGP ID + opt params len
    body = ph + local_addr + local_port + remote_port + open_msg + open_msg
    return bmp_header(3, body)


# ── Fixture 3: Route-Monitoring ANNOUNCE best-path (type=0) ──────────────────
def make_route_monitor_announce() -> bytes:
    ph = peer_header()
    # BGP UPDATE with NLRI 203.0.113.0/24
    # Proper BGP message: 16-byte marker, length, type
    marker = b"\xff" * 16
    nlri = b"\x18\xcb\x00\x71"  # /24 of 203.0.113.0
    # Minimal attributes: ORIGIN(IGP) + AS_PATH(65001) + NEXT_HOP(192.0.2.1)
    origin_attr = b"\x40\x01\x01\x00"                          # flags+type+len+IGP
    as_path_attr = b"\x40\x02\x06\x02\x01\x00\x00\xfd\xe9"    # SEQ, 1 AS, 65001
    next_hop_attr = b"\x40\x03\x04\xc0\x00\x02\x01"           # 192.0.2.1
    path_attrs = origin_attr + as_path_attr + next_hop_attr
    withdrawn_len = struct.pack("!H", 0)
    path_attrs_len = struct.pack("!H", len(path_attrs))
    bgp_body = withdrawn_len + path_attrs_len + path_attrs + nlri
    bgp_total_len = 16 + 2 + 1 + len(bgp_body)
    bgp_update = marker + struct.pack("!HB", bgp_total_len, 2) + bgp_body
    return bmp_header(0, ph + bgp_update)


# ── Fixture 4: Route-Monitoring WITHDRAW (type=0) ────────────────────────────
def make_route_monitor_withdraw() -> bytes:
    ph = peer_header()
    marker = b"\xff" * 16
    withdrawn_prefix = b"\x18\xcb\x00\x71"   # 203.0.113.0/24
    withdrawn_len = struct.pack("!H", len(withdrawn_prefix))
    path_attrs_len = struct.pack("!H", 0)
    bgp_body = withdrawn_len + withdrawn_prefix + path_attrs_len
    bgp_total_len = 16 + 2 + 1 + len(bgp_body)
    bgp_update = marker + struct.pack("!HB", bgp_total_len, 2) + bgp_body
    return bmp_header(0, ph + bgp_update)


# ── Fixture 5: Route-Monitoring with Path Status TLV best (type=0) ───────────
def make_route_monitor_path_status_best() -> bytes:
    ph = peer_header()
    marker = b"\xff" * 16
    nlri = b"\x18\xcb\x00\x71"
    origin_attr   = b"\x40\x01\x01\x00"
    as_path_attr  = b"\x40\x02\x06\x02\x01\x00\x00\xfd\xe9"
    next_hop_attr = b"\x40\x03\x04\xc0\x00\x02\x01"
    # Path Status TLV (RFC 9069): type=6, length=4 (4-byte status) + 2 (reason)
    # status=0x00000001 (BEST), reason=0x0000
    path_status_tlv = struct.pack("!HH", 6, 6) + struct.pack("!IH", 0x00000001, 0x0000)
    path_attrs = origin_attr + as_path_attr + next_hop_attr + path_status_tlv
    withdrawn_len   = struct.pack("!H", 0)
    path_attrs_len  = struct.pack("!H", len(path_attrs))
    bgp_body = withdrawn_len + path_attrs_len + path_attrs + nlri
    bgp_total_len = 16 + 2 + 1 + len(bgp_body)
    bgp_update = marker + struct.pack("!HB", bgp_total_len, 2) + bgp_body
    return bmp_header(0, ph + bgp_update)


# ── Fixture 6: Route-Monitoring with Path Status TLV nonselected (type=0) ────
def make_route_monitor_path_status_nonselected() -> bytes:
    ph = peer_header()
    marker = b"\xff" * 16
    nlri = b"\x18\xcb\x00\x71"
    origin_attr   = b"\x40\x01\x01\x00"
    as_path_attr  = b"\x40\x02\x06\x02\x01\x00\x00\xfd\xe9"
    next_hop_attr = b"\x40\x03\x04\xc0\x00\x02\x01"
    # status=0x00000004 (NONSELECTED), reason=0x0004 (AS_PATH length)
    path_status_tlv = struct.pack("!HH", 6, 6) + struct.pack("!IH", 0x00000004, 0x0004)
    path_attrs = origin_attr + as_path_attr + next_hop_attr + path_status_tlv
    withdrawn_len   = struct.pack("!H", 0)
    path_attrs_len  = struct.pack("!H", len(path_attrs))
    bgp_body = withdrawn_len + path_attrs_len + path_attrs + nlri
    bgp_total_len = 16 + 2 + 1 + len(bgp_body)
    bgp_update = marker + struct.pack("!HB", bgp_total_len, 2) + bgp_body
    return bmp_header(0, ph + bgp_update)


# ── Fixture 7: Peer-Down Notification (type=2) ───────────────────────────────
def make_peer_down() -> bytes:
    ph = peer_header()
    reason_code = struct.pack("!B", 3)   # reason=3: remote system closed
    body = ph + reason_code
    return bmp_header(2, body)


# ── Fixture 8: Statistics Report (type=1) ────────────────────────────────────
def make_stats_report() -> bytes:
    ph = peer_header()
    # Stats: 1 counter — type=0 (prefixes rejected), value=42
    stat_count = struct.pack("!I", 1)
    stat_type  = struct.pack("!HH", 0, 4)   # type=0, len=4
    stat_value = struct.pack("!I", 42)
    body = ph + stat_count + stat_type + stat_value
    return bmp_header(1, body)


# ── Fixture 9: Route-Monitoring IPv6 announce (type=0) ───────────────────────
def make_route_monitor_ipv6() -> bytes:
    ph = peer_header(peer_flags=0x80)    # flag bit 7 = IPv6
    marker = b"\xff" * 16
    # IPv6 NLRI: 2001:db8::/32
    nlri = b"\x20\x20\x01\x0d\xb8"      # /32 prefix, 4 bytes needed ceil(32/8)
    origin_attr  = b"\x40\x01\x01\x00"
    as_path_attr = b"\x40\x02\x06\x02\x01\x00\x00\xfd\xe9"
    path_attrs = origin_attr + as_path_attr
    withdrawn_len  = struct.pack("!H", 0)
    path_attrs_len = struct.pack("!H", len(path_attrs))
    bgp_body = withdrawn_len + path_attrs_len + path_attrs + nlri
    bgp_total_len = 16 + 2 + 1 + len(bgp_body)
    bgp_update = marker + struct.pack("!HB", bgp_total_len, 2) + bgp_body
    return bmp_header(0, ph + bgp_update)


# ── Fixture 10: Termination message (type=5) ─────────────────────────────────
def make_termination() -> bytes:
    # Reason TLV: type=0 (cease), value=0x0000 (administratively closed)
    reason_tlv = struct.pack("!HHH", 0, 2, 0)
    return bmp_header(5, reason_tlv)


# ── Main ─────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    print(f"Generating BMP fixtures → {os.path.abspath(FIXTURES_DIR)}")
    write("01_initiation.bin",                   make_initiation())
    write("02_peer_up.bin",                       make_peer_up())
    write("03_route_monitor_announce.bin",        make_route_monitor_announce())
    write("04_route_monitor_withdraw.bin",        make_route_monitor_withdraw())
    write("05_route_monitor_path_status_best.bin",        make_route_monitor_path_status_best())
    write("06_route_monitor_path_status_nonsel.bin",      make_route_monitor_path_status_nonselected())
    write("07_peer_down.bin",                     make_peer_down())
    write("08_stats_report.bin",                  make_stats_report())
    write("09_route_monitor_ipv6.bin",            make_route_monitor_ipv6())
    write("10_termination.bin",                   make_termination())
    print("Done.")
