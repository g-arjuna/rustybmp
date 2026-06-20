"""
RustyBMP policy fetcher — SSH-based NOS policy config retrieval.
RV7-V3 / §4.3 (adapted from bonsai/python/bootstrap_agent.py)

Security contract: credentials from RUSTYBMP_SSH_USERNAME / RUSTYBMP_SSH_PASSWORD
env vars. NEVER accepted as CLI args (visible in `ps -ef`) or HTTP body.

Vendors:
  Genie testbed (SSH): iosxr, iosxe, ios, nxos, junos, eos
  Paramiko raw SSH:    sros, srl, frr
"""
from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import time
from typing import Any

logger = logging.getLogger("policy_fetcher")

# ─── Vendor → Genie OS map (identical to bonsai bootstrap_agent.py os_map) ───

VENDOR_TO_GENIE_OS: dict[str, str] = {
    "iosxr": "iosxr", "vxr": "iosxr",
    "iosxe": "iosxe", "ios": "ios",
    "nxos": "nxos",
    "junos": "junos", "juniper": "junos",
    "eos": "eos", "arista": "eos",
}

PARAMIKO_VENDORS: set[str] = {
    "sros", "nokia_sros",
    "srl",  "nokia_srl",
    "frr",  "frrouting",
}

# ─── Credential helpers ───────────────────────────────────────────────────────

def _creds_from_env() -> tuple[str, str]:
    u = os.environ.get("RUSTYBMP_SSH_USERNAME", "")
    p = os.environ.get("RUSTYBMP_SSH_PASSWORD", "")
    if not u or not p:
        logger.error("RUSTYBMP_SSH_USERNAME / RUSTYBMP_SSH_PASSWORD not set")
        sys.exit(1)
    return u, p

# ─── Per-vendor command tables ────────────────────────────────────────────────

def _policy_commands(vendor: str, policy: str, peer: str, direction: str) -> list[str]:
    v = vendor.lower()
    if v in ("iosxr", "vxr"):
        return [
            f"show rpl route-policy {policy} detail",
            f"show bgp neighbor {peer} policy-statistics",
        ]
    elif v in ("iosxe", "ios", "nxos"):
        return [
            f"show route-map {policy}",
            f"show ip bgp neighbors {peer} policy",
        ]
    elif v in ("junos", "juniper"):
        return [
            f"show policy-options policy-statement {policy}",
            f"show policy-options policy-statement {policy} statistics",
        ]
    elif v in ("eos", "arista"):
        return [
            f"show route-map {policy}",
            f"show route-map {policy} statistics",
        ]
    elif v in ("sros", "nokia_sros"):
        return [f"show router policy-options policy {policy} statistics"]
    elif v in ("srl", "nokia_srl"):
        return [f"info /routing-policy policy {policy}"]
    elif v in ("frr", "frrouting"):
        return [f"show route-map {policy}"]
    else:
        return [f"show route-map {policy}"]

# ─── Genie (Tier 1) ──────────────────────────────────────────────────────────

def _connect_genie(address: str, username: str, password: str,
                   vendor: str, port: int):
    from genie.testbed import load as genie_load
    genie_os = VENDOR_TO_GENIE_OS.get(vendor.lower(), "iosxe")
    ssh_host  = address.split(":")[0]
    testbed_dict = {
        "devices": {
            address: {
                "os":   genie_os,
                "type": "router",
                "credentials": {"default": {"username": username, "password": password}},
                "connections": {"cli": {"protocol": "ssh", "ip": ssh_host, "port": port}},
            }
        }
    }
    testbed = genie_load(testbed_dict)
    device  = testbed.devices[address]
    device.connect(log_stdout=False)
    return device


def _run_genie(address: str, username: str, password: str,
               vendor: str, commands: list[str], port: int) -> dict[str, Any]:
    device = _connect_genie(address, username, password, vendor, port)
    try:
        result: dict[str, Any] = {}
        for cmd in commands:
            try:
                result[cmd] = {"structured": device.parse(cmd), "raw": None}
            except Exception:
                try:
                    result[cmd] = {"structured": None, "raw": device.execute(cmd)}
                except Exception as e:
                    result[cmd] = {"structured": None, "raw": None, "error": str(e)}
        return result
    finally:
        try:
            device.disconnect()
        except Exception:
            pass

# ─── Paramiko (fallback for SRL/FRR/SR-OS) ───────────────────────────────────

def _run_paramiko(address: str, username: str, password: str,
                  vendor: str, commands: list[str], port: int) -> dict[str, Any]:
    import paramiko
    ssh_host = address.split(":")[0]
    results: dict[str, Any] = {}
    client  = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    try:
        client.connect(
            ssh_host, port=port, username=username, password=password,
            timeout=30, look_for_keys=False, allow_agent=False,
        )
        for cmd in commands:
            # FRR routes through vtysh
            exec_cmd = f'vtysh -c "{cmd}"' if vendor in ("frr", "frrouting") else cmd
            try:
                _, stdout, _ = client.exec_command(exec_cmd, timeout=30)
                results[cmd] = {"structured": None, "raw": stdout.read().decode(errors="replace")}
            except Exception as e:
                results[cmd] = {"structured": None, "raw": None, "error": str(e)}
    finally:
        client.close()
    return results

# ─── Entry point ─────────────────────────────────────────────────────────────

def main() -> None:
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr)

    ap = argparse.ArgumentParser(description="rustybmp SSH policy fetcher")
    ap.add_argument("--peer-addr",  required=True,  help="Router IP / hostname")
    ap.add_argument("--vendor",     required=True,  help="iosxr|iosxe|junos|eos|nxos|sros|srl|frr")
    ap.add_argument("--policy",     required=True,  help="Policy / route-map name")
    ap.add_argument("--direction",  default="in",   help="in | out")
    ap.add_argument("--port",       type=int, default=22)
    args = ap.parse_args()

    # Credentials come ONLY from env vars
    username, password = _creds_from_env()

    t0       = time.time()
    commands = _policy_commands(args.vendor, args.policy, args.peer_addr, args.direction)
    parsed: dict[str, Any] = {}
    error = ""

    try:
        if args.vendor.lower() in PARAMIKO_VENDORS:
            parsed = _run_paramiko(
                args.peer_addr, username, password,
                args.vendor, commands, args.port,
            )
        else:
            parsed = _run_genie(
                args.peer_addr, username, password,
                args.vendor, commands, args.port,
            )
    except Exception as e:
        error = str(e)

    print(json.dumps({
        "peer_addr":    args.peer_addr,
        "vendor":       args.vendor,
        "policy_name":  args.policy,
        "direction":    args.direction,
        "status":       "ok" if not error else "failed",
        "error":        error,
        "commands":     parsed,
        "elapsed_s":    round(time.time() - t0, 2),
    }))


if __name__ == "__main__":
    main()
