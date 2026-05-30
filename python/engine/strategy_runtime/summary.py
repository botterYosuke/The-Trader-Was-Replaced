"""engine.strategy_runtime.summary — aggregate metrics for a strategy run-buffer."""
from __future__ import annotations

import json
import logging
import os
import tempfile
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)


def _coerce_float(value) -> Optional[float]:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


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
                continue
    return rows


def compute_summary(run_buffer_dir: Path) -> dict:
    """Compute aggregate metrics from equity.jsonl / fills.jsonl."""
    run_buffer_dir = Path(run_buffer_dir)
    equity_rows = _read_jsonl(run_buffer_dir / "equity.jsonl")
    fills_rows = _read_jsonl(run_buffer_dir / "fills.jsonl")

    equity_values: list[float] = []
    for row in equity_rows:
        v = _coerce_float(row.get("equity"))
        if v is not None:
            equity_values.append(v)

    if equity_values:
        total_pnl = equity_values[-1] - equity_values[0]
        peak = equity_values[0]
        max_dd = 0.0
        for v in equity_values:
            if v > peak:
                peak = v
            dd = peak - v
            if dd > max_dd:
                max_dd = dd
    else:
        total_pnl = 0.0
        max_dd = 0.0

    open_lots: list[tuple[str, float, float]] = []
    realised: list[float] = []
    fee_total = 0.0
    for row in fills_rows:
        raw_commission = row.get("commission")
        commission = _coerce_float(raw_commission)
        if commission is not None:
            fee_total += commission
        elif raw_commission not in (None, ""):
            log.warning(
                "summary: non-numeric commission ignored (value=%r) — fee_total may be understated",
                raw_commission,
            )
        side = row.get("side")
        qty = _coerce_float(row.get("qty"))
        price = _coerce_float(row.get("price"))
        if side not in ("BUY", "SELL") or qty is None or price is None or qty <= 0:
            continue
        opposite = "SELL" if side == "BUY" else "BUY"
        remaining = qty
        while remaining > 0 and open_lots and open_lots[0][0] == opposite:
            entry_side, entry_qty, entry_price = open_lots[0]
            close_qty = min(remaining, entry_qty)
            sign = 1.0 if entry_side == "BUY" else -1.0
            realised.append((price - entry_price) * sign * close_qty)
            entry_qty -= close_qty
            remaining -= close_qty
            if entry_qty <= 0:
                open_lots.pop(0)
            else:
                open_lots[0] = (entry_side, entry_qty, entry_price)
        if remaining > 0:
            open_lots.append((side, remaining, price))

    trade_count = len(realised)
    win_rate: Optional[float] = (
        sum(1 for r in realised if r > 0) / trade_count if trade_count > 0 else None
    )

    return {
        "total_pnl": total_pnl,
        "max_drawdown": max_dd,
        "trade_count": trade_count,
        "win_rate": win_rate,
        "fee_total": fee_total,
        "equity_points": len(equity_values),
        "fills_count": len(fills_rows),
    }


def equity_curve_stats(equity_values: list) -> dict:
    """Compute max_drawdown / sharpe / sortino from an in-memory equity curve."""
    import math

    n = len(equity_values)
    max_drawdown = 0.0
    sharpe = 0.0
    sortino = 0.0
    if n >= 2:
        peak = equity_values[0]
        for eq in equity_values:
            if eq > peak:
                peak = eq
            dd = peak - eq
            if dd > max_drawdown:
                max_drawdown = dd
        returns = [
            (equity_values[i] - equity_values[i - 1]) / equity_values[i - 1]
            for i in range(1, n)
            if equity_values[i - 1] != 0.0
        ]
        if returns:
            mean_r = sum(returns) / len(returns)
            variance = sum((r - mean_r) ** 2 for r in returns) / len(returns)
            std_r = math.sqrt(variance)
            sharpe = (mean_r / std_r) * math.sqrt(252) if std_r != 0.0 else 0.0
            neg_returns = [r for r in returns if r < 0.0]
            if neg_returns:
                neg_var = sum(r ** 2 for r in neg_returns) / len(neg_returns)
                downside_std = math.sqrt(neg_var)
                sortino = (mean_r / downside_std) * math.sqrt(252) if downside_std != 0.0 else 0.0
    return {"max_drawdown": max_drawdown, "sharpe": sharpe, "sortino": sortino}


def write_summary_json(target_dir: Path, summary: dict) -> Path:
    """Persist summary as summary.json under target_dir atomically."""
    target_dir = Path(target_dir)
    target_dir.mkdir(parents=True, exist_ok=True)
    target = target_dir / "summary.json"
    fd, tmp_path = tempfile.mkstemp(prefix="summary.", suffix=".json", dir=str(target_dir))
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            json.dump(summary, fh, ensure_ascii=False, indent=2)
        os.replace(tmp_path, target)
    except Exception:
        try:
            Path(tmp_path).unlink(missing_ok=True)
        except OSError:
            pass
        raise
    return target
