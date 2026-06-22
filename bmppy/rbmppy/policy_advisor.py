"""
Policy Recommendation Engine (RV9-NEW7)

Analyses filter accept/reject decisions alongside RPKI state and community
semantics to suggest Roto filter improvements.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any


class SuggestionKind(str, Enum):
    SHOULD_REJECT   = "should_reject"
    SHOULD_ACCEPT   = "should_accept"
    FALL_THROUGH    = "fall_through"
    EXPLAIN_REJECT  = "explain_reject"


@dataclass
class RouteCtx:
    prefix:        str
    peer_as:       int
    as_path:       list[int]
    communities:   list[str]
    rpki_state:    str           # "valid" | "invalid" | "not_found"
    aspa_verdict:  str           # "valid" | "invalid" | "unknown"
    rib_type:      str           # "pre-policy" | "post-policy" | "loc-rib"
    filter_action: str           # "accept" | "reject" | "fall_through"
    local_pref:    int | None    = None
    as_path_len:   int           = 0


@dataclass
class PolicySuggestion:
    kind:          SuggestionKind
    prefix:        str
    peer_as:       int
    explanation:   str
    roto_snippet:  str           # suggested Roto rule fragment
    confidence:    float         # 0.0 – 1.0
    evidence:      list[str]     = field(default_factory=list)


class PolicyAdvisor:
    """
    Analyse filter decisions and suggest improvements.

    Uses heuristics over RPKI state, community presence, ASPA verdicts,
    and AS path characteristics.  Does not require ML runtime — pure
    rule-based analysis for deterministic, auditable recommendations.
    """

    BLACKHOLE_COMMUNITIES = {"65535:666", "65535:0", "0:666"}
    PRIVATE_ASNS = set(range(64512, 65535)) | set(range(4200000000, 4294967295))

    def __init__(self, community_semantics: dict[str, str] | None = None):
        self._semantics: dict[str, str] = community_semantics or {}

    # ── Public API ────────────────────────────────────────────────────────────

    def analyze_filter_gaps(
        self,
        recent_routes: list[RouteCtx],
    ) -> list[PolicySuggestion]:
        """
        Detect routes that are accepted but probably shouldn't be,
        routes rejected that probably should be accepted, and routes
        that fall through to the implicit accept-all.
        """
        suggestions: list[PolicySuggestion] = []
        for route in recent_routes:
            suggestion = (
                self._check_should_reject(route)
                or self._check_should_accept(route)
                or self._check_fall_through(route)
            )
            if suggestion:
                suggestions.append(suggestion)
        return sorted(suggestions, key=lambda s: -s.confidence)

    def explain_rejection(self, route: RouteCtx) -> str:
        """
        Map a filter-rejected route to a human-readable explanation.
        """
        reasons: list[str] = []

        if route.rpki_state == "invalid":
            reasons.append("RPKI: route origin is INVALID (ROA mismatch)")
        if route.aspa_verdict == "invalid":
            reasons.append("ASPA: AS path fails provider-authorisation check")
        if any(asn in self.PRIVATE_ASNS for asn in route.as_path):
            private = [a for a in route.as_path if a in self.PRIVATE_ASNS]
            reasons.append(f"AS path contains private ASN(s): {private}")
        if route.as_path_len > 20:
            reasons.append(f"AS path too long: {route.as_path_len} hops (threshold 20)")
        blackhole = set(route.communities) & self.BLACKHOLE_COMMUNITIES
        if blackhole:
            reasons.append(f"Blackhole community detected: {blackhole}")
        if route.local_pref is not None and route.local_pref == 0:
            reasons.append("Local preference is 0 (BGP poison pill)")

        if not reasons:
            reasons.append("No explicit rule matched; rejected by implicit deny")

        return "; ".join(reasons)

    # ── Internal checks ───────────────────────────────────────────────────────

    def _check_should_reject(self, r: RouteCtx) -> PolicySuggestion | None:
        """Accepted route that probably should be rejected."""
        if r.filter_action != "accept":
            return None

        evidence: list[str] = []
        confidence = 0.0

        if r.rpki_state == "invalid":
            evidence.append("RPKI origin INVALID")
            confidence = max(confidence, 0.95)
        if r.aspa_verdict == "invalid":
            evidence.append("ASPA verdict INVALID")
            confidence = max(confidence, 0.90)
        private = [a for a in r.as_path if a in self.PRIVATE_ASNS]
        if private and r.rpki_state != "valid":
            evidence.append(f"private ASN(s) in path: {private}")
            confidence = max(confidence, 0.75)
        if r.as_path_len > 25:
            evidence.append(f"AS path unusually long ({r.as_path_len} hops)")
            confidence = max(confidence, 0.60)

        if not evidence:
            return None

        return PolicySuggestion(
            kind=SuggestionKind.SHOULD_REJECT,
            prefix=r.prefix,
            peer_as=r.peer_as,
            explanation=f"Route is accepted but has red flags: {'; '.join(evidence)}",
            roto_snippet=self._roto_reject_snippet(r, evidence),
            confidence=confidence,
            evidence=evidence,
        )

    def _check_should_accept(self, r: RouteCtx) -> PolicySuggestion | None:
        """Rejected route that probably should be accepted."""
        if r.filter_action != "reject":
            return None

        evidence: list[str] = []
        confidence = 0.0

        if r.rpki_state == "valid":
            evidence.append("RPKI origin is VALID")
            confidence = max(confidence, 0.80)
        well_known = set(r.communities) & set(self._semantics.keys())
        if well_known:
            labels = [self._semantics[c] for c in well_known]
            evidence.append(f"carries known-good communities: {labels}")
            confidence = max(confidence, 0.65)
        if r.aspa_verdict == "valid" and r.rpki_state == "valid":
            confidence = min(confidence + 0.10, 0.95)

        if not evidence:
            return None

        return PolicySuggestion(
            kind=SuggestionKind.SHOULD_ACCEPT,
            prefix=r.prefix,
            peer_as=r.peer_as,
            explanation=f"Route is rejected but appears legitimate: {'; '.join(evidence)}",
            roto_snippet=self._roto_accept_snippet(r, evidence),
            confidence=confidence,
            evidence=evidence,
        )

    def _check_fall_through(self, r: RouteCtx) -> PolicySuggestion | None:
        """Route matched no explicit rule (fell through to accept-all)."""
        if r.filter_action != "fall_through":
            return None

        return PolicySuggestion(
            kind=SuggestionKind.FALL_THROUGH,
            prefix=r.prefix,
            peer_as=r.peer_as,
            explanation=(
                "Route matched no explicit filter rule and was accepted by "
                "implicit fall-through. Consider adding an explicit rule."
            ),
            roto_snippet=self._roto_explicit_snippet(r),
            confidence=0.50,
            evidence=["no matching filter term found"],
        )

    # ── Roto snippet builders ─────────────────────────────────────────────────

    def _roto_reject_snippet(self, r: RouteCtx, reasons: list[str]) -> str:
        conditions: list[str] = []
        if "RPKI origin INVALID" in reasons:
            conditions.append("route.rpki == Invalid")
        if "ASPA verdict INVALID" in reasons:
            conditions.append("route.aspa == Invalid")
        if not conditions:
            conditions.append(f"route.prefix == {r.prefix}")
        cond = " && ".join(conditions)
        return (
            f"// Auto-suggested: reject ({'; '.join(reasons)})\n"
            f"filter {{\n"
            f"  if {cond} {{\n"
            f"    reject\n"
            f"  }}\n"
            f"}}"
        )

    def _roto_accept_snippet(self, r: RouteCtx, reasons: list[str]) -> str:
        return (
            f"// Auto-suggested: accept ({'; '.join(reasons)})\n"
            f"filter {{\n"
            f"  if route.rpki == Valid && route.peer_as == {r.peer_as} {{\n"
            f"    accept\n"
            f"  }}\n"
            f"}}"
        )

    def _roto_explicit_snippet(self, r: RouteCtx) -> str:
        return (
            f"// Auto-suggested: add explicit rule for AS{r.peer_as}\n"
            f"filter {{\n"
            f"  if route.peer_as == {r.peer_as} {{\n"
            f"    // TODO: define accept or reject criteria\n"
            f"    accept\n"
            f"  }}\n"
            f"}}"
        )
