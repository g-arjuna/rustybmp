"""
RV7 Policy AST — vendor-neutral representation of BGP routing policies.

A RouterPolicy (formerly called RouteMap on IOS) is a sequence of Terms.
Each Term has:
  - match_conditions : list of MatchCondition (prefix-list, community, AS-path ...)
  - set_actions      : list of SetAction (local-pref, community, next-hop ...)
  - action           : permit | deny

This AST is consumed by the PolicyCorrelator which compares it against
live BMP events to detect policy-advertisement divergence.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Optional


class TermAction(str, Enum):
    PERMIT = "permit"
    DENY   = "deny"


# ─── Match conditions ─────────────────────────────────────────────────────────

class MatchType(str, Enum):
    PREFIX_LIST       = "prefix-list"
    AS_PATH_FILTER    = "as-path-filter"
    COMMUNITY         = "community"
    LOCAL_PREF        = "local-pref"
    PEER_AS           = "peer-as"
    NEXT_HOP          = "next-hop"
    ROUTE_TYPE        = "route-type"
    TAG               = "tag"
    MED               = "med"
    OTHER             = "other"


@dataclass
class MatchCondition:
    match_type: MatchType
    value:      str        # string representation of the matched value / list name


# ─── Set actions ──────────────────────────────────────────────────────────────

class SetType(str, Enum):
    LOCAL_PREF       = "local-pref"
    COMMUNITY        = "community"
    COMMUNITY_ADDITIVE = "community-additive"
    COMMUNITY_DELETE = "community-delete"
    NEXT_HOP         = "next-hop"
    MED              = "med"
    AS_PATH_PREPEND  = "as-path-prepend"
    ORIGIN           = "origin"
    OTHER            = "other"


@dataclass
class SetAction:
    set_type: SetType
    value:    str


# ─── Policy term ──────────────────────────────────────────────────────────────

@dataclass
class RouteTerm:
    seq:              int
    action:           TermAction
    match_conditions: list[MatchCondition] = field(default_factory=list)
    set_actions:      list[SetAction]      = field(default_factory=list)
    description:      Optional[str]        = None

    def permits(self) -> bool:
        return self.action == TermAction.PERMIT

    def has_match(self, match_type: MatchType) -> bool:
        return any(c.match_type == match_type for c in self.match_conditions)

    def has_set(self, set_type: SetType) -> bool:
        return any(a.set_type == set_type for a in self.set_actions)


# ─── Route-map / policy container ────────────────────────────────────────────

@dataclass
class RouteMap:
    name:     str
    terms:    list[RouteTerm] = field(default_factory=list)

    def __len__(self) -> int:
        return len(self.terms)

    def permit_terms(self) -> list[RouteTerm]:
        return [t for t in self.terms if t.permits()]

    def deny_terms(self) -> list[RouteTerm]:
        return [t for t in self.terms if not t.permits()]

    def to_dict(self) -> dict:
        return {
            "name":  self.name,
            "terms": [
                {
                    "seq":    t.seq,
                    "action": t.action.value,
                    "match":  [{"type": c.match_type.value, "value": c.value}
                               for c in t.match_conditions],
                    "set":    [{"type": a.set_type.value, "value": a.value}
                               for a in t.set_actions],
                }
                for t in sorted(self.terms, key=lambda t: t.seq)
            ],
        }


# ─── Full policy AST for one router ──────────────────────────────────────────

@dataclass
class PolicyAst:
    """Complete parsed policy AST for a single router."""
    host:         str
    platform:     str
    route_maps:   list[RouteMap] = field(default_factory=list)

    def get(self, name: str) -> Optional[RouteMap]:
        for rm in self.route_maps:
            if rm.name == name:
                return rm
        return None

    def to_dict(self) -> dict:
        return {
            "host":       self.host,
            "platform":   self.platform,
            "route_maps": [rm.to_dict() for rm in self.route_maps],
        }
