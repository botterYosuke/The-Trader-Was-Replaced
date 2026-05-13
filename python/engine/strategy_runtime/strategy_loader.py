"""engine.strategy_runtime.strategy_loader — 戦略ファイルの動的ロード。

e-station の engine.nautilus.strategy_loader を元に、返り値を
``(module, scenario, strategy_cls)`` tuple に変更したもの。
インスタンス化は engine_runner 層が行う。

Public API:
    load(path) -> tuple[ModuleType, dict, type[Strategy]]
    get_strategy_param_env() -> dict[str, str]
    StrategyLoadError
"""

from __future__ import annotations

import ast
import importlib.util
import inspect
import logging
import os
import traceback
from pathlib import Path
from types import ModuleType
from typing import Any

log = logging.getLogger(__name__)

_INCOMPATIBLE_HANDLERS: frozenset[str] = frozenset(
    {"on_order_book_delta", "on_order_book_deltas", "on_quote_tick"}
)


class StrategyLoadError(Exception):
    pass


def load(path: str | Path) -> tuple[ModuleType, dict, Any]:  # Any = type[Strategy]
    """戦略 .py を読み込み ``(module, scenario, strategy_cls)`` を返す。

    - SCENARIO は ast.literal_eval で安全抽出し resolve_refs まで適用する。
    - strategy_cls はインスタンス化しない（呼び出し元 engine_runner の責務）。
    - ``STRATEGY_PARAM_*`` 環境変数の適用は ``get_strategy_param_env()`` で
      取得した dict を engine_runner が StrategyConfig / __init__ kwargs に注入する。

    Raises:
        FileNotFoundError: path が存在しない場合。
        StrategyLoadError: Strategy サブクラスが 0 個または複数個、あるいは import 失敗。
        ValueError: SCENARIO が見つからない、またはリテラル dict でない場合。
        ScenarioValidationError: resolve_refs での参照解決失敗。
    """
    from engine.strategy_runtime.scenario import extract, resolve_refs

    path = Path(path)
    if not path.exists():
        raise FileNotFoundError(f"strategy file not found: {path}")

    # モジュール名はファイルパス由来にして複数戦略の同時ロードに備える
    module_name = f"user_strategy_{path.stem}"

    spec = importlib.util.spec_from_file_location(module_name, path)
    if spec is None or spec.loader is None:
        raise StrategyLoadError(f"could not create module spec for {path}")

    module = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(module)  # type: ignore[union-attr]
    except Exception:
        raise StrategyLoadError(
            f"failed to import {path}:\n{traceback.format_exc()}"
        )

    # SCENARIO 抽出（モジュール実行後ではなく AST で安全抽出）
    from engine.strategy_runtime.scenario import validate
    scenario = extract(path)
    if scenario is None:
        raise ValueError(f"SCENARIO not found in {path}")
    scenario = resolve_refs(scenario, base_dir=path.parent)
    validate(scenario)

    # Strategy サブクラス検索（このファイルで定義されたものだけ）
    try:
        from nautilus_trader.trading.strategy import Strategy
    except ImportError as exc:
        raise StrategyLoadError(f"nautilus_trader not available: {exc}") from exc

    subclasses: list[type] = [
        cls
        for _name, cls in inspect.getmembers(module, inspect.isclass)
        if issubclass(cls, Strategy)
        and cls is not Strategy
        and cls.__module__ == module_name
    ]

    if len(subclasses) == 0:
        raise StrategyLoadError(f"no Strategy subclass found in {path}")
    if len(subclasses) > 1:
        names = ", ".join(cls.__name__ for cls in subclasses)
        raise StrategyLoadError(f"multiple Strategy subclasses found: [{names}]")

    strategy_cls = subclasses[0]
    _check_compat(path)
    return module, scenario, strategy_cls


def get_strategy_param_env() -> dict[str, str]:
    """``STRATEGY_PARAM_*`` 環境変数を ``{lower_key: value_str}`` として返す。

    例: ``STRATEGY_PARAM_HOLDING_MINUTES=42``
        → ``{"holding_minutes": "42"}``

    engine_runner が StrategyConfig フィールドや __init__ kwargs へのキャストと
    注入を行う。
    """
    prefix = "STRATEGY_PARAM_"
    return {
        k[len(prefix):].lower(): v
        for k, v in os.environ.items()
        if k.startswith(prefix)
    }


def _check_compat(path: Path) -> None:
    """非互換ハンドラ（on_order_book_delta 等）を AST スキャンして warn する。"""
    src = path.read_text(encoding="utf-8")
    tree = ast.parse(src)
    for node in ast.walk(tree):
        if (
            isinstance(node, ast.FunctionDef)
            and node.name in _INCOMPATIBLE_HANDLERS
        ):
            log.warning(
                "strategy file %s defines '%s' which is not supported in replay mode "
                "(TradeTick/OrderBookDelta feeds are not available). "
                "This handler will not be called.",
                path,
                node.name,
            )
