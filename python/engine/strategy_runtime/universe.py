"""engine.strategy_runtime.universe — UNIVERSE_JSON_PATH resolution helper.

Public API:
    resolve_universe_json_path(strategy_path, universe_json_path) -> Path
"""
from __future__ import annotations

from pathlib import Path


def resolve_universe_json_path(
    strategy_path: str | Path,
    universe_json_path: str | Path,
) -> Path:
    """Resolve universe_json_path relative to the strategy file if not absolute.

    order_flow_06.on_start uses ``Path(__file__).parent / relative_path``.
    This function replicates that logic so the CLI can pass an absolute path
    (which the strategy accepts without re-resolving).

    Args:
        strategy_path: Path to the strategy .py file.
        universe_json_path: Value from UNIVERSE_JSON_PATH in the strategy
            (may be relative like ``"../data/universe/foo.json"``).

    Returns:
        Absolute resolved Path.
    """
    p = Path(universe_json_path)
    if p.is_absolute():
        return p
    return (Path(strategy_path).parent / p).resolve()
