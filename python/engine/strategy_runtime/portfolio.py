"""engine.strategy_runtime.portfolio — build a portfolio snapshot from run-buffer files."""
from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)


def _read_jsonl(path: Path) -> list[dict]:
    rows: list[dict] = []
    if not path.exists():
        return rows
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return rows


def compute_portfolio(run_dir: Path, scenario: Optional[dict] = None) -> dict:
    """Build a portfolio snapshot dict from fills.jsonl and equity.jsonl.

    Returns:
        {buying_power, cash, equity, positions: list[dict], orders: list[dict]}
    """
    run_dir = Path(run_dir)
    fills = _read_jsonl(run_dir / "fills.jsonl")
    equity_rows = _read_jsonl(run_dir / "equity.jsonl")

    # ── orders (= all fills) ──────────────────────────────────────────────────
    orders: list[dict] = [
        {
            "symbol": row.get("instrument_id", ""),
            "side": row.get("side", ""),
            "qty": _to_float(row.get("qty")),
            "price": _to_float(row.get("price")),
            "status": "FILLED",
            "ts_ms": int(row.get("ts_event_ms", 0)),
        }
        for row in fills
    ]

    # ── positions (net from fills) ────────────────────────────────────────────
    net: dict[str, dict] = {}
    for row in fills:
        sym = row.get("instrument_id", "")
        qty = _to_float(row.get("qty"))
        price = _to_float(row.get("price"))
        side = row.get("side", "")
        if sym not in net:
            net[sym] = {"signed_qty": 0.0, "cost": 0.0}
        sign = 1.0 if side == "BUY" else -1.0
        net[sym]["signed_qty"] += sign * qty
        net[sym]["cost"] += sign * qty * price

    positions: list[dict] = []
    for sym, d in net.items():
        sq = d["signed_qty"]
        if abs(sq) > 1e-9:
            avg = d["cost"] / sq
            positions.append({
                "symbol": sym,
                "qty": int(round(sq)),
                "avg_price": avg,
                "unrealized_pnl": 0.0,
            })

    # ── equity ────────────────────────────────────────────────────────────────
    equity_vals = [_to_float(r.get("equity")) for r in equity_rows if "equity" in r]
    initial_cash = float((scenario or {}).get("initial_cash", 0) or 0)
    last_equity = equity_vals[-1] if equity_vals else initial_cash

    return {
        "buying_power": last_equity,
        "cash": last_equity,
        "equity": last_equity,
        "positions": positions,
        "orders": orders,
    }


def _to_float(value) -> float:
    if value is None:
        return 0.0
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0
