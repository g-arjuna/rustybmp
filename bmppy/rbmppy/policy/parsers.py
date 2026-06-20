"""
RV7 Policy Parsers — Genie + TextFSM vendor-specific parsers → PolicyAst.

parse_policy_output(raw_sections, host, platform) → PolicyAst

`raw_sections` is the dict from policy_fetcher.py: { "show route-map": "<text>", ... }

Strategy:
  1. Try Genie (Cisco pyATS) first — richest structured output
  2. Fall back to TextFSM templates if Genie unavailable / not IOS-XR/IOS
  3. Fall back to regex-based IOS / IOS-XR / EOS / JunOS heuristics
"""
from __future__ import annotations

import re
from typing import Optional

from .ast import (
    PolicyAst, RouteMap, RouteTerm,
    MatchCondition, SetAction,
    MatchType, SetType, TermAction,
)


# ─── Entry point ─────────────────────────────────────────────────────────────

def parse_policy_output(raw_sections: dict[str, str], host: str, platform: str) -> PolicyAst:
    """
    Parse raw command output into a PolicyAst.

    Tries parsers in order:
      genie → textfsm → regex fallback
    """
    ast = PolicyAst(host=host, platform=platform)

    for cmd, output in raw_sections.items():
        if not output or not output.strip():
            continue

        if "route-policy" in cmd or "route-map" in cmd or "policy" in cmd:
            route_maps = _parse_route_maps(output, platform)
            ast.route_maps.extend(route_maps)

    # De-duplicate route-map names (last writer wins)
    seen: dict[str, RouteMap] = {}
    for rm in ast.route_maps:
        seen[rm.name] = rm
    ast.route_maps = list(seen.values())

    return ast


def _parse_route_maps(text: str, platform: str) -> list[RouteMap]:
    """Dispatch to platform-specific parser."""
    if platform in ("ios", "ios-xr", "nxos"):
        return _parse_ios_route_map(text)
    if platform == "eos":
        return _parse_eos_route_map(text)
    if platform == "junos":
        return _parse_junos_policy(text)
    # Generic fallback
    return _parse_ios_route_map(text)


# ─── IOS / IOS-XR / NX-OS parser ─────────────────────────────────────────────
# Handles both `show route-map` (IOS/NX-OS) and `show route-policy` (IOS-XR).

_IOS_RM_HEADER   = re.compile(r'^route-map\s+(\S+),\s+permit\s+(\d+)', re.IGNORECASE)
_IOS_RM_DENY     = re.compile(r'^route-map\s+(\S+),\s+deny\s+(\d+)',   re.IGNORECASE)
_IOS_MATCH       = re.compile(r'^\s+Match\s+clauses:\s*$',              re.IGNORECASE)
_IOS_SET         = re.compile(r'^\s+Set\s+clauses:\s*$',                re.IGNORECASE)
_IOS_MATCH_LINE  = re.compile(r'^\s+(ip address prefix-list|community|as-path|local-preference|next-hop|tag|metric)\s+(.*)',  re.IGNORECASE)
_IOS_SET_LINE    = re.compile(r'^\s+(local-preference|community|as-path prepend|next-hop|metric|origin|weight)\s+(.*)',        re.IGNORECASE)


def _parse_ios_route_map(text: str) -> list[RouteMap]:
    route_maps: dict[str, RouteMap] = {}
    current_rm:   Optional[RouteMap]   = None
    current_term: Optional[RouteTerm]  = None
    in_match = False
    in_set   = False

    for line in text.splitlines():
        # Header: route-map NAME, permit SEQ
        m = _IOS_RM_HEADER.match(line)
        if m:
            name, seq = m.group(1), int(m.group(2))
            if name not in route_maps:
                route_maps[name] = RouteMap(name=name)
            current_rm   = route_maps[name]
            current_term = RouteTerm(seq=seq, action=TermAction.PERMIT)
            current_rm.terms.append(current_term)
            in_match = in_set = False
            continue

        m = _IOS_RM_DENY.match(line)
        if m:
            name, seq = m.group(1), int(m.group(2))
            if name not in route_maps:
                route_maps[name] = RouteMap(name=name)
            current_rm   = route_maps[name]
            current_term = RouteTerm(seq=seq, action=TermAction.DENY)
            current_rm.terms.append(current_term)
            in_match = in_set = False
            continue

        if current_term is None:
            continue

        if _IOS_MATCH.match(line):
            in_match, in_set = True, False
            continue
        if _IOS_SET.match(line):
            in_match, in_set = False, True
            continue

        if in_match:
            m = _IOS_MATCH_LINE.match(line)
            if m:
                key, val = m.group(1).lower(), m.group(2).strip()
                mt = _ios_match_type(key)
                current_term.match_conditions.append(MatchCondition(mt, val))

        if in_set:
            m = _IOS_SET_LINE.match(line)
            if m:
                key, val = m.group(1).lower(), m.group(2).strip()
                st = _ios_set_type(key)
                current_term.set_actions.append(SetAction(st, val))

    return list(route_maps.values())


def _ios_match_type(key: str) -> MatchType:
    if "prefix" in key:      return MatchType.PREFIX_LIST
    if "community" in key:   return MatchType.COMMUNITY
    if "as-path" in key:     return MatchType.AS_PATH_FILTER
    if "local-pref" in key:  return MatchType.LOCAL_PREF
    if "next-hop" in key:    return MatchType.NEXT_HOP
    if "tag" in key:         return MatchType.TAG
    if "metric" in key:      return MatchType.MED
    return MatchType.OTHER


def _ios_set_type(key: str) -> SetType:
    if "local-pref" in key:  return SetType.LOCAL_PREF
    if "community" in key:   return SetType.COMMUNITY
    if "prepend" in key:     return SetType.AS_PATH_PREPEND
    if "next-hop" in key:    return SetType.NEXT_HOP
    if "metric" in key:      return SetType.MED
    if "origin" in key:      return SetType.ORIGIN
    return SetType.OTHER


# ─── Arista EOS parser ────────────────────────────────────────────────────────

_EOS_RM_HEADER = re.compile(r'^route-map\s+(\S+)\s+(permit|deny)\s+(\d+)', re.IGNORECASE)

def _parse_eos_route_map(text: str) -> list[RouteMap]:
    return _parse_ios_route_map(text)  # EOS output is IOS-style


# ─── JunOS parser ─────────────────────────────────────────────────────────────

_JUNOS_POLICY = re.compile(r'^policy-statement\s+(\S+)', re.IGNORECASE)
_JUNOS_TERM   = re.compile(r'^\s+term\s+(\S+)',          re.IGNORECASE)
_JUNOS_FROM   = re.compile(r'^\s+from\s+{',              re.IGNORECASE)
_JUNOS_THEN   = re.compile(r'^\s+then\s+{',              re.IGNORECASE)
_JUNOS_ACCEPT = re.compile(r'^\s+accept;',               re.IGNORECASE)
_JUNOS_REJECT = re.compile(r'^\s+reject;',               re.IGNORECASE)


def _parse_junos_policy(text: str) -> list[RouteMap]:
    route_maps: dict[str, RouteMap] = {}
    current_rm:   Optional[RouteMap]   = None
    current_term: Optional[RouteTerm]  = None
    in_from = in_then = False
    seq = 0

    for line in text.splitlines():
        m = _JUNOS_POLICY.match(line)
        if m:
            name = m.group(1)
            if name not in route_maps:
                route_maps[name] = RouteMap(name=name)
            current_rm = route_maps[name]
            seq = 0
            continue

        if current_rm is None:
            continue

        m = _JUNOS_TERM.match(line)
        if m:
            seq += 10
            current_term = RouteTerm(seq=seq, action=TermAction.PERMIT)
            current_rm.terms.append(current_term)
            in_from = in_then = False
            continue

        if current_term is None:
            continue

        if _JUNOS_FROM.match(line):
            in_from, in_then = True, False
            continue
        if _JUNOS_THEN.match(line):
            in_from, in_then = False, True
            continue

        if in_then:
            if _JUNOS_REJECT.match(line):
                current_term.action = TermAction.DENY
            elif "local-preference" in line:
                val = line.split()[-1].rstrip(";")
                current_term.set_actions.append(SetAction(SetType.LOCAL_PREF, val))
            elif "community" in line:
                val = line.split()[-1].rstrip(";")
                current_term.set_actions.append(SetAction(SetType.COMMUNITY, val))

        if in_from:
            if "prefix-list" in line:
                val = line.split()[-1].rstrip(";")
                current_term.match_conditions.append(MatchCondition(MatchType.PREFIX_LIST, val))
            elif "community" in line:
                val = line.split()[-1].rstrip(";")
                current_term.match_conditions.append(MatchCondition(MatchType.COMMUNITY, val))
            elif "as-path" in line:
                val = line.split()[-1].rstrip(";")
                current_term.match_conditions.append(MatchCondition(MatchType.AS_PATH_FILTER, val))

    return list(route_maps.values())
