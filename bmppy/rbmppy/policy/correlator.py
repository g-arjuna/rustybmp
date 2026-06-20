"""
RV7 Policy Correlator — correlate PolicyAst against live BMP route events.

Detects:
  1. Routes advertised despite a policy DENY term matching them
  2. Routes blocked (absent from DuckDB) despite a policy PERMIT term allowing them
  3. Communities set by policy but absent from received routes
  4. LOCAL_PREF divergence: policy sets X but received route shows Y

Usage:
    correlator = PolicyCorrelator(duckdb_path="runtime/routes.duckdb")
    findings = correlator.correlate(policy_ast, peer_addr="10.1.1.1")
"""
from __future__ import annotations

import ipaddress
from dataclasses import dataclass, field
from typing import Optional

import duckdb

from .ast import PolicyAst, MatchType, SetType, TermAction


# ─── Finding types ────────────────────────────────────────────────────────────

@dataclass
class PolicyFinding:
    severity:    str    # "info" | "warn" | "critical"
    category:    str    # "leaked_route" | "blocked_route" | "community_mismatch" | "localpref_drift"
    prefix:      str
    peer_addr:   str
    description: str
    route_map:   Optional[str] = None
    term_seq:    Optional[int] = None


# ─── Correlator ───────────────────────────────────────────────────────────────

class PolicyCorrelator:
    """
    Correlates a PolicyAst against the live route_events table in DuckDB.

    Args:
        duckdb_path: path to the DuckDB database (or ":memory:" for tests).
    """

    def __init__(self, duckdb_path: str = "runtime/routes.duckdb"):
        self._db_path = duckdb_path

    def _conn(self) -> duckdb.DuckDBPyConnection:
        return duckdb.connect(self._db_path, read_only=True)

    def correlate(self, ast: PolicyAst, peer_addr: str, hours: int = 1) -> list[PolicyFinding]:
        """
        Run all correlation checks and return a list of findings.
        """
        findings: list[PolicyFinding] = []

        with self._conn() as conn:
            live_routes = self._load_live_routes(conn, peer_addr, hours)

        findings += self._check_leaked_routes(ast, live_routes, peer_addr)
        findings += self._check_blocked_routes(ast, live_routes, peer_addr)
        findings += self._check_community_mismatch(ast, live_routes, peer_addr)
        findings += self._check_localpref_drift(ast, live_routes, peer_addr)

        return findings

    # ── Data load ────────────────────────────────────────────────────────────

    def _load_live_routes(self, conn, peer_addr: str, hours: int) -> list[dict]:
        """Load latest announced routes per prefix for the given peer."""
        sql = f"""
            WITH latest AS (
                SELECT prefix, as_path, local_pref, communities, action,
                       ROW_NUMBER() OVER (PARTITION BY prefix ORDER BY occurred_at DESC) AS rn
                FROM route_events
                WHERE peer_addr = '{peer_addr}'
                  AND occurred_at >= NOW() - INTERVAL '{hours} hours'
            )
            SELECT prefix, as_path, local_pref, communities
            FROM latest
            WHERE rn = 1 AND action = 'announce'
        """
        try:
            rows = conn.execute(sql).fetchall()
            return [
                {"prefix": r[0], "as_path": r[1], "local_pref": r[2], "communities": r[3]}
                for r in rows
            ]
        except Exception:
            return []

    # ── Checks ────────────────────────────────────────────────────────────────

    def _check_leaked_routes(
        self, ast: PolicyAst, live_routes: list[dict], peer_addr: str
    ) -> list[PolicyFinding]:
        """Find routes that are live but should be denied by the policy."""
        findings = []
        live_prefixes = {r["prefix"] for r in live_routes}

        for rm in ast.route_maps:
            for term in rm.deny_terms():
                for cond in term.match_conditions:
                    if cond.match_type != MatchType.PREFIX_LIST:
                        continue
                    # We only have the list name, not the prefixes — flag for review
                    for prefix in live_prefixes:
                        if _prefix_in_list_hint(prefix, cond.value):
                            findings.append(PolicyFinding(
                                severity   = "critical",
                                category   = "leaked_route",
                                prefix     = prefix,
                                peer_addr  = peer_addr,
                                description= (
                                    f"Route {prefix} is live but route-map '{rm.name}' "
                                    f"term {term.seq} DENY matches prefix-list '{cond.value}'"
                                ),
                                route_map  = rm.name,
                                term_seq   = term.seq,
                            ))

        return findings

    def _check_blocked_routes(
        self, ast: PolicyAst, live_routes: list[dict], peer_addr: str
    ) -> list[PolicyFinding]:
        """Find permit terms whose prefix-list is entirely absent from live routes — potential block."""
        findings = []
        live_prefixes = {r["prefix"] for r in live_routes}

        for rm in ast.route_maps:
            for term in rm.permit_terms():
                for cond in term.match_conditions:
                    if cond.match_type != MatchType.PREFIX_LIST:
                        continue
                    # If zero live routes match the list hint, flag it as potentially blocked
                    matched = [p for p in live_prefixes if _prefix_in_list_hint(p, cond.value)]
                    if not matched and cond.value not in ("", "any"):
                        findings.append(PolicyFinding(
                            severity   = "warn",
                            category   = "blocked_route",
                            prefix     = f"(prefix-list {cond.value})",
                            peer_addr  = peer_addr,
                            description= (
                                f"route-map '{rm.name}' term {term.seq} PERMIT references "
                                f"prefix-list '{cond.value}' but no matching routes received"
                            ),
                            route_map  = rm.name,
                            term_seq   = term.seq,
                        ))

        return findings

    def _check_community_mismatch(
        self, ast: PolicyAst, live_routes: list[dict], peer_addr: str
    ) -> list[PolicyFinding]:
        """Find routes where a SET community action differs from the received communities."""
        findings = []

        for rm in ast.route_maps:
            for term in rm.permit_terms():
                for action in term.set_actions:
                    if action.set_type not in (SetType.COMMUNITY, SetType.COMMUNITY_ADDITIVE):
                        continue
                    expected_comm = action.value.strip()
                    for route in live_routes:
                        comms = route.get("communities") or ""
                        if expected_comm and expected_comm not in comms:
                            findings.append(PolicyFinding(
                                severity   = "warn",
                                category   = "community_mismatch",
                                prefix     = route["prefix"],
                                peer_addr  = peer_addr,
                                description= (
                                    f"route-map '{rm.name}' term {term.seq} sets community "
                                    f"'{expected_comm}' but received route has: '{comms}'"
                                ),
                                route_map  = rm.name,
                                term_seq   = term.seq,
                            ))

        return findings

    def _check_localpref_drift(
        self, ast: PolicyAst, live_routes: list[dict], peer_addr: str
    ) -> list[PolicyFinding]:
        """Find routes where the received LOCAL_PREF diverges from the policy SET."""
        findings = []

        for rm in ast.route_maps:
            for term in rm.permit_terms():
                for action in term.set_actions:
                    if action.set_type != SetType.LOCAL_PREF:
                        continue
                    try:
                        expected_lp = int(action.value.split()[0])
                    except (ValueError, IndexError):
                        continue

                    for route in live_routes:
                        received_lp = route.get("local_pref")
                        if received_lp is not None and received_lp != expected_lp:
                            findings.append(PolicyFinding(
                                severity   = "warn",
                                category   = "localpref_drift",
                                prefix     = route["prefix"],
                                peer_addr  = peer_addr,
                                description= (
                                    f"route-map '{rm.name}' term {term.seq} sets "
                                    f"LOCAL_PREF={expected_lp} but received={received_lp}"
                                ),
                                route_map  = rm.name,
                                term_seq   = term.seq,
                            ))

        return findings


# ─── Helpers ──────────────────────────────────────────────────────────────────

def _prefix_in_list_hint(prefix: str, list_name: str) -> bool:
    """
    Heuristic: if the list_name looks like a CIDR range, check containment.
    Otherwise return False (we don't have the full prefix-list expansion).
    """
    try:
        net = ipaddress.ip_network(list_name, strict=False)
        pfx = ipaddress.ip_network(prefix,    strict=False)
        return net.supernet_of(pfx) or net == pfx
    except ValueError:
        return False
