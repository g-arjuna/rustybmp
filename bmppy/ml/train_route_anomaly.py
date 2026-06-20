"""Train Model A — IsolationForest for BGP route anomaly detection (RV4-4 T2).

Adapted from bonsai/python/train_anomaly.py architecture.

Workflow::

    # 1. Export training data
    python -m rbmppy.parquet --db runtime/routes.duckdb \\
                              --out ml/data --days 7

    # 2. Train anomaly model
    python bmppy/ml/train_route_anomaly.py \\
        --input ml/data/routes_7d.parquet \\
        --output ml/models/route_anomaly_v1.joblib

    # 3. Integrate into detection pipeline
    from bmppy.ml.train_route_anomaly import RouteAnomalyModel
    model = RouteAnomalyModel.load("ml/models/route_anomaly_v1.joblib")
    scores = model.score_df(df)   # -1 = anomalous, 1 = normal
"""
from __future__ import annotations

import argparse
from pathlib import Path
from datetime import datetime, timezone
from typing import Optional

try:
    import pandas as pd
    import numpy as np
    from sklearn.ensemble import IsolationForest
    from sklearn.preprocessing import StandardScaler
    from sklearn.pipeline import Pipeline
    import joblib
except ImportError as e:
    raise ImportError(
        "scikit-learn, pandas, numpy, and joblib are required. "
        "Install with: pip install scikit-learn pandas numpy joblib"
    ) from e

FEATURE_COLS = [
    "hop_count",
    "origin_asn",
    "is_announce",
    "local_pref",
    "med",
    "community_count",
    "rpki_enc",
    "occurred_at_s",
]


class RouteAnomalyModel:
    """Wrapper around a trained IsolationForest pipeline."""

    def __init__(self, pipeline: Pipeline, trained_at: datetime, n_samples: int) -> None:
        self.pipeline   = pipeline
        self.trained_at = trained_at
        self.n_samples  = n_samples

    @classmethod
    def train(
        cls,
        df: "pd.DataFrame",
        n_estimators: int = 200,
        contamination: float = 0.05,
        random_state: int = 42,
    ) -> "RouteAnomalyModel":
        """Train on a feature DataFrame. Returns a fitted model."""
        X = df[FEATURE_COLS].fillna(0).astype(float)
        pipeline = Pipeline([
            ("scaler", StandardScaler()),
            ("iso_forest", IsolationForest(
                n_estimators=n_estimators,
                contamination=contamination,
                random_state=random_state,
                n_jobs=-1,
            )),
        ])
        pipeline.fit(X)
        return cls(pipeline, datetime.now(timezone.utc), len(X))

    @classmethod
    def load(cls, path: str) -> "RouteAnomalyModel":
        """Load a previously saved model."""
        obj = joblib.load(path)
        if not isinstance(obj, cls):
            raise TypeError(f"Expected RouteAnomalyModel, got {type(obj)}")
        return obj

    def save(self, path: str) -> None:
        """Persist model to disk using joblib."""
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        joblib.dump(self, path)

    def predict(self, df: "pd.DataFrame") -> "np.ndarray":
        """Return IsolationForest predictions: 1 = normal, -1 = anomalous."""
        X = df[FEATURE_COLS].fillna(0).astype(float)
        return self.pipeline.predict(X)

    def score_df(self, df: "pd.DataFrame") -> "pd.DataFrame":
        """Return input DataFrame with added 'anomaly_score' and 'is_anomaly' columns."""
        X      = df[FEATURE_COLS].fillna(0).astype(float)
        scores = self.pipeline.named_steps["iso_forest"].score_samples(
            self.pipeline.named_steps["scaler"].transform(X)
        )
        result = df.copy()
        result["anomaly_score"] = scores
        result["is_anomaly"]    = self.pipeline.predict(X) == -1
        return result


def main() -> None:
    parser = argparse.ArgumentParser(description="Train BGP route anomaly IsolationForest")
    parser.add_argument("--input",  required=True, help="Input Parquet file (from rbmppy.parquet)")
    parser.add_argument("--output", default="ml/models/route_anomaly_v1.joblib")
    parser.add_argument("--estimators",    type=int,   default=200)
    parser.add_argument("--contamination", type=float, default=0.05)
    args = parser.parse_args()

    print(f"Loading {args.input} …")
    df = pd.read_parquet(args.input, columns=FEATURE_COLS)
    print(f"  {len(df):,} rows, {df.shape[1]} features")

    print("Training IsolationForest …")
    model = RouteAnomalyModel.train(
        df,
        n_estimators=args.estimators,
        contamination=args.contamination,
    )

    model.save(args.output)
    print(f"Model saved → {args.output}")
    print(f"  trained_at  : {model.trained_at.isoformat()}")
    print(f"  n_samples   : {model.n_samples:,}")
    print(f"  contamination: {args.contamination}")

    # Quick sanity: score first 10 rows
    sample = df.head(10)
    scored = model.score_df(sample)
    anomalies = scored["is_anomaly"].sum()
    print(f"  Sanity check (first 10 rows): {anomalies} anomalies flagged")


if __name__ == "__main__":
    main()
