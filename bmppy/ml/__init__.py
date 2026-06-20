"""RustyBMP ML pipeline (RV4-4).

Modules:
  parquet_store      — rolling Parquet archive with latest symlinks
  train_route_anomaly — IsolationForest on route feature matrix
  topology_snapshot  — BGP peer graph snapshots for STGNN training
"""
