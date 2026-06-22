"""
Bundle D3 — Hijack Probability Classifier.

Replaces heuristic-only hijack detection with a GradientBoostingClassifier
trained on labeled BGP hijack features.

Features (7):
  1. origin_asn_changed    — bool: did the origin AS change?
  2. prefix_specificity    — int:  CIDR prefix length (e.g. 24 for /24)
  3. rpki_validity_enc     — int:  0=valid, 1=not_found, 2=invalid
  4. as_path_len_delta     — int:  current - previous AS-path length
  5. aspa_verdict_enc      — int:  0=valid, 1=unknown, 2=invalid
  6. is_subprefix_of_known — bool: is this a more-specific of a known prefix?
  7. peer_as_is_expected   — bool: is the peer AS in the expected set?

Targets: recall > 0.95, AUC >= 0.90 on validation set.

Falls back to the existing heuristic detector (detectors.py BGPHijackDetector)
when no trained model is available.

Usage::

    from bmppy.ml.hijack_classifier import HijackClassifier
    clf = HijackClassifier.load("ml/models/hijack_gbc_v1.pkl")
    prob = clf.predict_proba(features)
"""
from __future__ import annotations

import logging
import pickle
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import numpy as np

logger = logging.getLogger(__name__)

FEATURE_NAMES = [
    "origin_asn_changed",
    "prefix_specificity",
    "rpki_validity_enc",
    "as_path_len_delta",
    "aspa_verdict_enc",
    "is_subprefix_of_known",
    "peer_as_is_expected",
]

RPKI_ENC = {"valid": 0, "not_found": 1, "invalid": 2}
ASPA_ENC = {"valid": 0, "unknown": 1, "invalid": 2}


@dataclass
class HijackFeatures:
    """Feature vector for a single route event."""

    origin_asn_changed: bool = False
    prefix_specificity: int = 24
    rpki_validity_enc: int = 1
    as_path_len_delta: int = 0
    aspa_verdict_enc: int = 1
    is_subprefix_of_known: bool = False
    peer_as_is_expected: bool = True

    def to_array(self) -> np.ndarray:
        return np.array([
            int(self.origin_asn_changed),
            self.prefix_specificity,
            self.rpki_validity_enc,
            self.as_path_len_delta,
            self.aspa_verdict_enc,
            int(self.is_subprefix_of_known),
            int(self.peer_as_is_expected),
        ], dtype=np.float64)


@dataclass
class HijackPrediction:
    """Result of hijack classification."""

    probability: float
    is_hijack: bool
    model_version: str = "heuristic"


class HijackClassifier:
    """GradientBoosting hijack classifier with heuristic fallback.

    Parameters
    ----------
    model_path:
        Path to a pickled sklearn GradientBoostingClassifier.
        If None or file doesn't exist, uses heuristic scoring.
    threshold:
        Probability threshold for classifying as hijack.
    """

    def __init__(
        self,
        model_path: Optional[str] = None,
        threshold: float = 0.5,
    ) -> None:
        self._model = None
        self._model_version = "heuristic"
        self._threshold = threshold

        if model_path and Path(model_path).exists():
            try:
                with open(model_path, "rb") as f:
                    self._model = pickle.load(f)
                self._model_version = f"gbc:{Path(model_path).stem}"
                logger.info("Loaded hijack classifier: %s", model_path)
            except Exception as exc:
                logger.warning("Failed to load model %s: %s — using heuristic", model_path, exc)

    @classmethod
    def load(cls, path: str, threshold: float = 0.5) -> "HijackClassifier":
        """Convenience constructor."""
        return cls(model_path=path, threshold=threshold)

    @property
    def is_ml(self) -> bool:
        """True if using a trained ML model rather than heuristic."""
        return self._model is not None

    def predict(self, features: HijackFeatures) -> HijackPrediction:
        """Classify a single route event."""
        x = features.to_array().reshape(1, -1)

        if self._model is not None:
            try:
                prob = float(self._model.predict_proba(x)[0, 1])
            except Exception as exc:
                logger.debug("ML prediction failed, falling back: %s", exc)
                prob = self._heuristic_score(features)
        else:
            prob = self._heuristic_score(features)

        return HijackPrediction(
            probability=prob,
            is_hijack=prob >= self._threshold,
            model_version=self._model_version,
        )

    def predict_batch(self, feature_list: list[HijackFeatures]) -> list[HijackPrediction]:
        """Classify a batch of events."""
        return [self.predict(f) for f in feature_list]

    @staticmethod
    def _heuristic_score(f: HijackFeatures) -> float:
        """Rule-based fallback when no ML model is available.

        Scoring (0.0–1.0):
          +0.30 if origin changed
          +0.25 if RPKI invalid
          +0.15 if sub-prefix of known
          +0.10 if AS-path shortened by 3+
          +0.10 if ASPA invalid
          -0.20 if peer AS is expected
          -0.15 if RPKI valid
        """
        score = 0.0
        if f.origin_asn_changed:
            score += 0.30
        if f.rpki_validity_enc == 2:  # invalid
            score += 0.25
        elif f.rpki_validity_enc == 0:  # valid
            score -= 0.15
        if f.is_subprefix_of_known:
            score += 0.15
        if f.as_path_len_delta <= -3:
            score += 0.10
        if f.aspa_verdict_enc == 2:  # invalid
            score += 0.10
        if f.peer_as_is_expected:
            score -= 0.20
        return max(0.0, min(1.0, score))

    @staticmethod
    def train_and_save(
        X: np.ndarray,
        y: np.ndarray,
        output_path: str,
        n_estimators: int = 200,
        max_depth: int = 4,
        learning_rate: float = 0.1,
    ) -> dict:
        """Train a GBC model and save to disk.

        Parameters
        ----------
        X : array of shape (n_samples, 7)
        y : array of shape (n_samples,) — binary labels (1=hijack)
        output_path : where to save the .pkl
        n_estimators, max_depth, learning_rate : GBC hyperparameters

        Returns
        -------
        dict with 'auc', 'recall', 'precision', 'n_samples'.
        """
        from sklearn.ensemble import GradientBoostingClassifier
        from sklearn.model_selection import train_test_split
        from sklearn.metrics import roc_auc_score, recall_score, precision_score

        X_train, X_val, y_train, y_val = train_test_split(
            X, y, test_size=0.2, random_state=42, stratify=y,
        )

        gbc = GradientBoostingClassifier(
            n_estimators=n_estimators,
            max_depth=max_depth,
            learning_rate=learning_rate,
            random_state=42,
        )
        gbc.fit(X_train, y_train)

        y_proba = gbc.predict_proba(X_val)[:, 1]
        y_pred = (y_proba >= 0.5).astype(int)

        metrics = {
            "auc": float(roc_auc_score(y_val, y_proba)),
            "recall": float(recall_score(y_val, y_pred)),
            "precision": float(precision_score(y_val, y_pred)),
            "n_samples": len(y),
        }

        Path(output_path).parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "wb") as f:
            pickle.dump(gbc, f)

        logger.info(
            "Hijack classifier saved → %s  (AUC=%.3f recall=%.3f precision=%.3f n=%d)",
            output_path, metrics["auc"], metrics["recall"],
            metrics["precision"], metrics["n_samples"],
        )
        return metrics
