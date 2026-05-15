"""engine.strategy_runtime.scenario — SCENARIO 定数の安全抽出・検証。

e-station の engine.scenario から write_back / libcst / LIVE_SCENARIO / path guard を
除いた replay-only サブセット。公開 API:

    extract(path)                       -> Optional[dict]
    normalize_scenario(d)               -> dict
    load_scenario(strategy_path)        -> dict
    validate(d)                         -> None
    ScenarioValidationError
"""

from __future__ import annotations

import ast
import json
import logging
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Sidecar path helper
# ---------------------------------------------------------------------------


def _sidecar_path(strategy_path: Path) -> Path:
    """foo.py → foo.json（stem ベースで拡張子を .json に換える）。
    foo.bar.py → foo.bar.json（stem = "foo.bar"）。
    """
    return strategy_path.with_name(strategy_path.stem + ".json")


_NON_LITERAL_ERROR = (
    "dict literal 以外（unpacking {**...} / comprehension / 関数呼び出しを含む dict）は "
    "SCENARIO として読めません。リテラルの dict だけを使ってください"
)

# ---------------------------------------------------------------------------
# Public exception
# ---------------------------------------------------------------------------


class ScenarioValidationError(Exception):
    def __init__(self, message: str, *, code: str | None = None) -> None:
        super().__init__(message)
        self.code = code


# ---------------------------------------------------------------------------
# extract
# ---------------------------------------------------------------------------


def extract(path: Path) -> Optional[dict]:  # type: ignore[type-arg]
    """path の .py から SCENARIO 定数を ast.literal_eval で安全抽出する。

    import は一切発火しない（副作用ゼロ）。
    AnnAssign 形と Assign 形の両方を許容。
    """
    source = path.read_text(encoding="utf-8")
    tree = ast.parse(source, filename=str(path))
    found: Optional[dict] = None  # type: ignore[type-arg]

    for node in ast.iter_child_nodes(tree):
        scenario_value: Optional[ast.expr] = None

        if isinstance(node, ast.Assign):
            if (
                len(node.targets) == 1
                and isinstance(node.targets[0], ast.Name)
                and node.targets[0].id == "SCENARIO"
            ):
                scenario_value = node.value

        elif isinstance(node, ast.AnnAssign):
            if isinstance(node.target, ast.Name) and node.target.id == "SCENARIO":
                if node.value is None:
                    continue
                scenario_value = node.value

        if scenario_value is not None:
            if isinstance(scenario_value, ast.DictComp):
                raise ValueError(_NON_LITERAL_ERROR)
            if not isinstance(scenario_value, ast.Dict):
                raise ValueError(_NON_LITERAL_ERROR)
            if any(k is None for k in scenario_value.keys):
                raise ValueError(_NON_LITERAL_ERROR)
            try:
                result = ast.literal_eval(scenario_value)
            except (ValueError, TypeError) as exc:
                raise ValueError(_NON_LITERAL_ERROR) from exc
            if not isinstance(result, dict):
                raise ValueError(_NON_LITERAL_ERROR)
            if found is not None:
                raise ScenarioValidationError(
                    "multiple SCENARIO assignments are not supported"
                )
            found = result

    if found is not None:
        log.info("scenario.extract path=%s keys=%d", path, len(found))
    return found


# ---------------------------------------------------------------------------
# validate
# ---------------------------------------------------------------------------

_V1_TYPES: dict[str, type] = {
    "schema_version": int,
    "instrument": str,
    "start": str,
    "end": str,
    "granularity": str,
    "initial_cash": int,
}
_V2_TYPES: dict[str, type] = {
    "schema_version": int,
    "instruments": list,
    "start": str,
    "end": str,
    "granularity": str,
    "initial_cash": int,
}
_V3_TYPES: dict[str, type] = {
    "schema_version": int,
    "instruments": list,
    "start": str,
    "end": str,
    "granularity": str,
    "initial_cash": int,
}
_V3_OPTIONAL: frozenset[str] = frozenset({"strategy_init_kwargs"})


def _check_keys(
    d: dict,  # type: ignore[type-arg]
    required: frozenset[str],
    optional: frozenset[str],
) -> None:
    missing = required - d.keys()
    if missing:
        raise ScenarioValidationError(
            f"SCENARIO missing required keys: {sorted(missing)}"
        )
    extra = d.keys() - required - optional
    if extra:
        raise ScenarioValidationError(f"SCENARIO has unknown keys: {sorted(extra)}")


def _check_types(d: dict, expected: dict[str, type]) -> None:  # type: ignore[type-arg]
    for key, expected_type in expected.items():
        val = d[key]
        if isinstance(val, bool) and expected_type is int:
            raise ScenarioValidationError(f"SCENARIO[{key!r}] must be int, got bool")
        if not isinstance(val, expected_type):
            raise ScenarioValidationError(
                f"SCENARIO[{key!r}] must be {expected_type.__name__}, "
                f"got {type(val).__name__}"
            )


def _check_str_list(d: dict, key: str) -> None:  # type: ignore[type-arg]
    lst = d[key]
    if len(lst) == 0:
        raise ScenarioValidationError(f"SCENARIO[{key!r}] must not be empty")
    for i, item in enumerate(lst):
        if not isinstance(item, str):
            raise ScenarioValidationError(
                f"SCENARIO[{key!r}][{i}] must be str, got {type(item).__name__}"
            )


def validate(d: dict) -> None:  # type: ignore[type-arg]
    """Scenario dict の runtime 検証。失敗時は ScenarioValidationError を raise。

    v3 の optional key（strategy_init_kwargs）は型 unchecked（実利用側が解釈する）。
    """
    if not isinstance(d, dict):
        raise ScenarioValidationError(f"SCENARIO must be a dict, got {type(d).__name__}")

    sv = d.get("schema_version")
    # NOTE: normalize_scenario() が呼び出し元（load_scenario / strategy_loader）で
    # 適用済みであることを前提とする。ここでの重複正規化は削除済み。
    if sv == 1:
        _check_keys(d, frozenset(_V1_TYPES), frozenset())
        _check_types(d, _V1_TYPES)
    elif sv == 2:
        _check_keys(d, frozenset(_V2_TYPES), frozenset())
        _check_types(d, {k: v for k, v in _V2_TYPES.items() if k != "instruments"})
        _check_str_list(d, "instruments")
    elif sv == 3:
        _check_keys(d, frozenset(_V3_TYPES), _V3_OPTIONAL)
        _check_types(d, {k: v for k, v in _V3_TYPES.items() if k != "instruments"})
        _check_str_list(d, "instruments")
    else:
        raise ScenarioValidationError(
            f"SCENARIO schema_version must be 1, 2 or 3, got {sv!r}"
        )


# ---------------------------------------------------------------------------
# normalize_scenario
# ---------------------------------------------------------------------------


def normalize_scenario(d: dict) -> dict:  # type: ignore[type-arg]
    """v2/v3 の "instrument"（単数）キーを "instruments"（複数）に正規化した新 dict を返す。

    既に正規化済みの dict はそのまま返す（idempotent）。
    v1 は "instrument" キーが正当なので変換しない。
    """
    sv = d.get("schema_version")
    if sv in (2, 3) and "instrument" in d and "instruments" not in d:
        out = dict(d)
        out["instruments"] = out.pop("instrument")
        return out
    return d


# ---------------------------------------------------------------------------
# load_scenario
# ---------------------------------------------------------------------------


def load_scenario(strategy_path: Path) -> dict:  # type: ignore[type-arg]
    """サイドカー <strategy>.json の "scenario" キーを返す。

    必ず normalize_scenario → validate の順で通す。

    フォールバック順:
      1. <strategy>.json が存在し "scenario" キーがある → JSON ロード
      2. <strategy>.json が存在するが "scenario" キーがない → .py にフォールバック
      3. <strategy>.py 内に SCENARIO がある → extract() に委譲 + WARN ログ
      4. どちらも無ければ ValueError

    Raises:
        ScenarioValidationError: サイドカー JSON が壊れている場合
        ValueError: SCENARIO が見つからない場合
    """
    sidecar = _sidecar_path(strategy_path)
    if sidecar.exists():
        try:
            doc = json.loads(sidecar.read_text(encoding="utf-8"))
        except json.JSONDecodeError as exc:
            raise ScenarioValidationError(f"invalid JSON in {sidecar}: {exc}") from exc
        if isinstance(doc, dict) and "scenario" in doc:
            d = doc["scenario"]
            d = normalize_scenario(d)
            validate(d)
            return d
        # サイドカーはあるが "scenario" キーが無い（layout-only サイドカー）
        # → .py にフォールバック

    if strategy_path.exists():
        d = extract(strategy_path)
        if d is not None:
            log.warning(
                "SCENARIO loaded from .py (legacy); migrate to %s",
                sidecar.name,
            )
            d = normalize_scenario(d)
            validate(d)
            return d

    raise ValueError(
        f"SCENARIO not found: looked for 'scenario' key in {sidecar} "
        f"and SCENARIO in {strategy_path}"
    )
