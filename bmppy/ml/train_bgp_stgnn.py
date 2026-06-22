"""Train Model B — GATv2-GRU Spatio-Temporal GNN for BGP anomaly detection (RV5-9).

Architecture (based on IMACSI 2025):
  Spatial:  GATv2Conv — learns per-peer attention-weighted neighbourhood features
  Temporal: GRU over a T=8 snapshot sequence (5-minute intervals)
  Output:   Per-node anomaly score (regression) or binary label (classification)

Workflow::

    # 1. Export prefix aggregates
    python -m rbmppy.analytics --db runtime/routes.duckdb \\
                                --export-aggregates ml/data/prefix_agg_7d.parquet

    # 2. Build topology snapshots
    python bmppy/ml/train_bgp_stgnn.py build-snapshots \\
        --db runtime/routes.duckdb \\
        --out ml/data/snapshots.arrow \\
        --T 8

    # 3. Train STGNN
    python bmppy/ml/train_bgp_stgnn.py train \\
        --snapshots ml/data/snapshots.arrow \\
        --output ml/models/bgp_stgnn_v1.pt \\
        --epochs 50 --lr 1e-3

    # 4. Score live snapshots
    python bmppy/ml/train_bgp_stgnn.py score \\
        --db runtime/routes.duckdb \\
        --model ml/models/bgp_stgnn_v1.pt
"""
from __future__ import annotations

import argparse
import logging
import sys
from pathlib import Path
from datetime import datetime, timezone
from typing import Optional

logger = logging.getLogger(__name__)

# ─── Constants ────────────────────────────────────────────────────────────────

NUM_NODE_FEATURES  = 11  # must match topology_snapshot.NODE_FEATURE_COLS
HIDDEN_DIM         = 64
GRU_LAYERS         = 2
GAT_HEADS          = 4
DEFAULT_T          = 8
INTERVAL_MINUTES   = 5


# ─── Model definition (lazy-imported so script works without torch on import) ─

def _build_model(num_features: int, hidden: int, heads: int, gru_layers: int):
    """Build the GATv2-GRU STGNN model.  Requires torch + torch-geometric."""
    try:
        import torch
        import torch.nn as nn
        from torch_geometric.nn import GATv2Conv
    except ImportError as e:
        raise ImportError(
            "torch and torch-geometric required: "
            "pip install torch torch-geometric"
        ) from e

    class BgpSTGNN(nn.Module):
        def __init__(self):
            super().__init__()
            self.gat1 = GATv2Conv(num_features, hidden, heads=heads, concat=True)
            self.gat2 = GATv2Conv(hidden * heads, hidden, heads=1,     concat=False)
            self.gru  = nn.GRU(hidden, hidden, num_layers=gru_layers, batch_first=True)
            self.fc   = nn.Linear(hidden, 1)

        def forward(self, snapshot_list):
            """
            snapshot_list: list of (x, edge_index) tensors length T.
            Returns per-node anomaly scores of shape (N,).
            """
            node_embeds = []
            for x, edge_index in snapshot_list:
                h = torch.relu(self.gat1(x, edge_index))
                h = torch.relu(self.gat2(h, edge_index))
                node_embeds.append(h)  # (N, hidden)

            # Stack → (N, T, hidden) then run GRU
            stacked = torch.stack(node_embeds, dim=1)  # (N, T, hidden)
            gru_out, _ = self.gru(stacked)              # (N, T, hidden)
            last = gru_out[:, -1, :]                    # (N, hidden)
            return self.fc(last).squeeze(-1)            # (N,)

    return BgpSTGNN()


# ─── Commands ──────────────────────────────────────────────────────────────────

def cmd_build_snapshots(args) -> None:
    """Build a SnapshotSequence from live DuckDB and save as Arrow IPC."""
    try:
        from rbmppy.analytics import RouteAnalytics
        from bmppy.ml.topology_snapshot import SnapshotSequence
    except ImportError as e:
        sys.exit(f"Import error: {e}")

    analytics = RouteAnalytics(args.db)
    logger.info("Building %d snapshots (interval=%dmin) …", args.T, INTERVAL_MINUTES)
    seq = SnapshotSequence.build(analytics, T=args.T, interval_minutes=INTERVAL_MINUTES)
    logger.info("Snapshots: %s", [s.summary() for s in seq.snapshots])
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    seq.save_arrow(args.out)
    logger.info("Saved → %s", args.out)
    analytics.close()


def cmd_train(args) -> None:
    """Train the GATv2-GRU STGNN from a saved Arrow snapshot file."""
    try:
        import torch
        import torch.nn as nn
        import torch.optim as optim
        import pyarrow.ipc as ipc
        import numpy as np
        from bmppy.ml.topology_snapshot import SnapshotSequence, BgpTopologySnapshot
        from rbmppy.analytics import RouteAnalytics
    except ImportError as e:
        sys.exit(f"Import error: {e}")

    logger.info("Loading snapshots from %s …", args.snapshots)
    # Rebuild from DB (simpler than re-serialising the full graph structure)
    analytics = RouteAnalytics(args.db)
    seq       = SnapshotSequence.build(analytics, T=args.T, interval_minutes=INTERVAL_MINUTES)
    analytics.close()

    pyg_list = seq.to_pyg()
    if not pyg_list:
        sys.exit("No snapshots returned; is the database populated?")

    logger.info("Building model …")
    model     = _build_model(NUM_NODE_FEATURES, HIDDEN_DIM, GAT_HEADS, GRU_LAYERS)
    optimizer = optim.Adam(model.parameters(), lr=args.lr)
    criterion = nn.MSELoss()

    # Self-supervised: predict next-step node features from previous steps
    # Label = norm of node feature vector (proxy for activity level)
    model.train()
    for epoch in range(1, args.epochs + 1):
        snapshot_input = [
            (d.x, d.edge_index)
            for d in pyg_list
        ]
        # Target: L2-norm of last snapshot features per node
        target = pyg_list[-1].x.norm(dim=1)

        optimizer.zero_grad()
        scores = model(snapshot_input)
        loss   = criterion(scores, target)
        loss.backward()
        optimizer.step()

        if epoch % max(1, args.epochs // 10) == 0 or epoch == 1:
            logger.info("Epoch %4d/%d — loss=%.6f", epoch, args.epochs, loss.item())

    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    torch.save({
        "model_state": model.state_dict(),
        "trained_at":  datetime.now(timezone.utc).isoformat(),
        "num_features": NUM_NODE_FEATURES,
        "hidden_dim":   HIDDEN_DIM,
        "gat_heads":    GAT_HEADS,
        "gru_layers":   GRU_LAYERS,
    }, args.output)
    logger.info("Model saved → %s", args.output)


def cmd_score(args) -> None:
    """Score a live snapshot sequence and print per-peer anomaly scores."""
    try:
        import torch
        from rbmppy.analytics import RouteAnalytics
        from bmppy.ml.topology_snapshot import SnapshotSequence
    except ImportError as e:
        sys.exit(f"Import error: {e}")

    ckpt = torch.load(args.model, map_location="cpu")
    model = _build_model(
        ckpt["num_features"], ckpt["hidden_dim"],
        ckpt["gat_heads"],    ckpt["gru_layers"],
    )
    model.load_state_dict(ckpt["model_state"])
    model.eval()

    analytics = RouteAnalytics(args.db)
    seq       = SnapshotSequence.build(analytics, T=DEFAULT_T, interval_minutes=INTERVAL_MINUTES)
    pyg_list  = seq.to_pyg()

    with torch.no_grad():
        snapshot_input = [(d.x, d.edge_index) for d in pyg_list]
        scores = model(snapshot_input).numpy()

    # Map back to peer addresses
    last_snap = seq.snapshots[-1]
    idx_to_peer = {v: k for k, v in last_snap.node_index.items()}

    print(f"\n{'Peer':<22} {'Score':>8}  Anomaly?")
    print("-" * 40)
    threshold = float(scores.mean()) + 2.0 * float(scores.std())
    for i, score in enumerate(scores):
        peer = idx_to_peer.get(i, f"node_{i}")
        flag = "⚠ ANOMALY" if score > threshold else ""
        print(f"{peer:<22} {score:>8.4f}  {flag}")

    analytics.close()


# ─── CLI ──────────────────────────────────────────────────────────────────────

def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s  %(message)s")
    p = argparse.ArgumentParser(description="BGP STGNN training and scoring")
    sub = p.add_subparsers(dest="cmd", required=True)

    # build-snapshots
    bs = sub.add_parser("build-snapshots", help="Build Arrow snapshot file from DuckDB")
    bs.add_argument("--db",  required=True)
    bs.add_argument("--out", default="ml/data/snapshots.arrow")
    bs.add_argument("--T",   type=int, default=DEFAULT_T)

    # train
    tr = sub.add_parser("train", help="Train the GATv2-GRU STGNN")
    tr.add_argument("--db",        required=True)
    tr.add_argument("--snapshots", default="ml/data/snapshots.arrow")
    tr.add_argument("--output",    default="ml/models/bgp_stgnn_v1.pt")
    tr.add_argument("--epochs",    type=int,   default=50)
    tr.add_argument("--lr",        type=float, default=1e-3)
    tr.add_argument("--T",         type=int,   default=DEFAULT_T)

    # score
    sc = sub.add_parser("score", help="Score live snapshots with a trained model")
    sc.add_argument("--db",    required=True)
    sc.add_argument("--model", required=True)

    args = p.parse_args()
    {"build-snapshots": cmd_build_snapshots, "train": cmd_train, "score": cmd_score}[args.cmd](args)


if __name__ == "__main__":
    main()
