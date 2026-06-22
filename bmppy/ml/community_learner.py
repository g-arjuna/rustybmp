"""
Bundle D5 — Community Semantics Learner.

Uses fpgrowth (mlxtend) to discover frequent itemsets in BGP community
attribute co-occurrences with policy actions, then infers semantic meanings.

Example output::

    [
      {"community": "65001:100", "inferred_meaning": "preferred transit (LP=200)", "confidence": 0.94},
      {"community": "65001:200", "inferred_meaning": "backup path (LP=80)",        "confidence": 0.87},
      {"community": "65001:666", "inferred_meaning": "blackhole",                  "confidence": 0.99},
    ]

Usage::

    from bmppy.ml.community_learner import CommunitySemanticLearner
    learner = CommunitySemanticLearner(db_url="http://localhost:7878")
    results = await learner.learn()
"""
from __future__ import annotations

import logging
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Optional

import numpy as np

logger = logging.getLogger(__name__)

UTC = timezone.utc

# Known semantic patterns — community values that map to well-known meanings
_WELL_KNOWN_PATTERNS = {
    "blackhole": ["666", ":666", "rtbh", "blackhole"],
    "no-export": ["65535:65281", "no-export"],
    "no-advertise": ["65535:65282", "no-advertise"],
    "no-peer": ["65535:65284", "nopeer"],
}

# Local-pref thresholds for semantic inference
_LP_HIGH = 150  # "preferred"
_LP_LOW = 80    # "backup"


@dataclass
class CommunitySemantics:
    """Inferred semantic meaning for a BGP community value."""

    community: str
    inferred_meaning: str
    confidence: float
    support: float = 0.0
    route_count: int = 0
    first_seen: Optional[str] = None
    last_seen: Optional[str] = None


class CommunitySemanticLearner:
    """Learns community semantics from pre/post policy correlations.

    This learner works in two phases:
    1. **Pattern matching**: Maps well-known community values (e.g. x:666 → blackhole).
    2. **Statistical inference**: Uses community co-occurrence with policy
       effects (local-pref changes, route filtering) to infer meanings.

    Parameters
    ----------
    min_support:
        Minimum support threshold for fpgrowth (fraction of transactions).
    min_confidence:
        Minimum confidence for an inference to be reported.
    """

    def __init__(
        self,
        min_support: float = 0.05,
        min_confidence: float = 0.70,
    ) -> None:
        self._min_support = min_support
        self._min_confidence = min_confidence
        self._results: list[CommunitySemantics] = []

    @property
    def results(self) -> list[CommunitySemantics]:
        return list(self._results)

    def learn_from_dataframe(self, df: "pd.DataFrame") -> list[CommunitySemantics]:
        """Learn community semantics from a DataFrame.

        Expected columns: ``community``, ``local_pref``, ``action``,
        ``pre_policy`` (bool), ``route_count``.

        Parameters
        ----------
        df:
            DataFrame with community-route associations.

        Returns
        -------
        List of inferred semantic meanings.
        """
        import pandas as pd

        results: list[CommunitySemantics] = []

        if df.empty:
            logger.info("No community data to learn from")
            return results

        # Phase 1: Well-known patterns
        for community in df["community"].unique():
            comm_lower = str(community).lower()
            for meaning, patterns in _WELL_KNOWN_PATTERNS.items():
                if any(p in comm_lower for p in patterns):
                    subset = df[df["community"] == community]
                    results.append(CommunitySemantics(
                        community=str(community),
                        inferred_meaning=meaning,
                        confidence=0.99,
                        support=len(subset) / len(df),
                        route_count=int(subset["route_count"].sum()) if "route_count" in subset.columns else len(subset),
                    ))
                    break

        known_communities = {r.community for r in results}

        # Phase 2: Statistical inference from policy effects
        for community in df["community"].unique():
            if str(community) in known_communities:
                continue

            subset = df[df["community"] == community]
            if len(subset) < 3:
                continue

            support = len(subset) / len(df)
            if support < self._min_support:
                continue

            # Check local-pref correlation
            if "local_pref" in subset.columns:
                lp_values = subset["local_pref"].dropna()
                if len(lp_values) > 0:
                    median_lp = float(lp_values.median())
                    lp_consistency = 1.0 - float(lp_values.std() / (lp_values.mean() + 1e-6)) if len(lp_values) > 1 else 1.0
                    lp_consistency = max(0.0, min(1.0, lp_consistency))

                    if median_lp >= _LP_HIGH and lp_consistency >= self._min_confidence:
                        results.append(CommunitySemantics(
                            community=str(community),
                            inferred_meaning=f"preferred path (LP={int(median_lp)})",
                            confidence=lp_consistency,
                            support=support,
                            route_count=len(subset),
                        ))
                        known_communities.add(str(community))
                        continue

                    if median_lp <= _LP_LOW and lp_consistency >= self._min_confidence:
                        results.append(CommunitySemantics(
                            community=str(community),
                            inferred_meaning=f"backup path (LP={int(median_lp)})",
                            confidence=lp_consistency,
                            support=support,
                            route_count=len(subset),
                        ))
                        known_communities.add(str(community))
                        continue

            # Check if community correlates with route filtering
            if "action" in subset.columns:
                withdraw_ratio = (subset["action"] == "withdraw").mean()
                if withdraw_ratio >= 0.8 and withdraw_ratio >= self._min_confidence:
                    results.append(CommunitySemantics(
                        community=str(community),
                        inferred_meaning="route filter / deny",
                        confidence=float(withdraw_ratio),
                        support=support,
                        route_count=len(subset),
                    ))
                    known_communities.add(str(community))
                    continue

        self._results = results
        logger.info("Learned %d community semantics", len(results))
        return results

    def learn_from_itemsets(self, df: "pd.DataFrame") -> list[CommunitySemantics]:
        """Use fpgrowth to discover frequent community co-occurrence patterns.

        Parameters
        ----------
        df:
            Boolean-encoded transaction DataFrame where each column is a
            community value and each row is a route event.

        Returns
        -------
        List of inferred semantics from frequent itemsets.
        """
        try:
            from mlxtend.frequent_patterns import fpgrowth, association_rules
        except ImportError:
            logger.warning("mlxtend not installed — skipping fpgrowth analysis")
            return []

        if df.empty or df.shape[1] < 2:
            return []

        frequent = fpgrowth(df, min_support=self._min_support, use_colnames=True)
        if frequent.empty:
            return []

        rules = association_rules(frequent, metric="confidence", min_threshold=self._min_confidence)

        results: list[CommunitySemantics] = []
        for _, rule in rules.iterrows():
            antecedents = ", ".join(sorted(rule["antecedents"]))
            consequents = ", ".join(sorted(rule["consequents"]))
            results.append(CommunitySemantics(
                community=antecedents,
                inferred_meaning=f"co-occurs with {consequents}",
                confidence=float(rule["confidence"]),
                support=float(rule["support"]),
            ))

        self._results.extend(results)
        return results
