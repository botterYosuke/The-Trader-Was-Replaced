"""engine.strategy_runtime.scenario — SCENARIO 定数の安全抽出・参照解決・検証。

e-station の engine.scenario から write_back / libcst / LIVE_SCENARIO / path guard を
除いた replay-only サブセット。公開 API:

    extract(path)                  -> Optional[dict]
    resolve_refs(d, *, base_dir)   -> dict
    validate(d)                    -> None
    ScenarioValidationError
"""

from __future__ import annotations

import ast
import json
import logging
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)

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
# resolve_refs
# ---------------------------------------------------------------------------


def _resolve_json_pointer(doc: object, pointer: str) -> object:
    if pointer in ("", "#"):
        return doc
    if pointer.startswith("#/"):
        pointer = pointer[1:]
    if not pointer.startswith("/"):
        raise ScenarioValidationError(
            f"Invalid JSON Pointer: {pointer!r}", code="unresolved_ref"
        )
    tokens = pointer[1:].split("/")
    current = doc
    for token in tokens:
        token = token.replace("~1", "/").replace("~0", "~")
        try:
            if isinstance(current, list):
                current = current[int(token)]
            elif isinstance(current, dict):
                current = current[token]  # type: ignore[index]
            else:
                raise ScenarioValidationError(
                    f"JSON Pointer traversal failed at {token!r}: not a dict or list",
                    code="unresolved_ref",
                )
        except (KeyError, IndexError, ValueError) as exc:
            raise ScenarioValidationError(
                f"JSON Pointer traversal failed at {token!r}: {exc}",
                code="unresolved_ref",
            ) from exc
    return current


def resolve_refs(d: dict, *, base_dir: Path) -> dict:  # type: ignore[type-arg]
    """v3 の instruments_ref を解決して instruments を追加した新 dict を返す。v1/v2 は no-op。"""
    if d.get("schema_version") != 3:
        return dict(d)

    if "instruments" in d and "instruments_ref" in d:
        raise ScenarioValidationError(
            "SCENARIO['instruments'] and ['instruments_ref'] cannot coexist"
        )

    if "instruments_ref" not in d:
        return dict(d)

    ref: str = d["instruments_ref"]

    if "#" in ref:
        path_part, pointer_part = ref.split("#", 1)
        pointer_part = "#" + pointer_part
    else:
        path_part = ref
        pointer_part = ""

    if not path_part:
        raise ScenarioValidationError(
            "instruments_ref with empty path (self-reference) is not supported",
            code="unresolved_ref",
        )

    try:
        file_path = base_dir / path_part
        raw = file_path.read_text(encoding="utf-8")
        doc = json.loads(raw)
    except OSError as exc:
        raise ScenarioValidationError(
            f"instruments_ref: cannot read {path_part!r}: {exc}",
            code="unresolved_ref",
        ) from exc
    except json.JSONDecodeError as exc:
        raise ScenarioValidationError(
            f"instruments_ref: invalid JSON in {path_part!r}: {exc}",
            code="unresolved_ref",
        ) from exc

    resolved = _resolve_json_pointer(doc, pointer_part)

    if not isinstance(resolved, list):
        raise ScenarioValidationError(
            f"instruments_ref resolved to {type(resolved).__name__}, expected list[str]",
            code="unresolved_ref",
        )
    for i, item in enumerate(resolved):
        if not isinstance(item, str):
            raise ScenarioValidationError(
                f"instruments_ref resolved list[{i}] must be str, got {type(item).__name__}",
                code="unresolved_ref",
            )

    result = dict(d)
    result["instruments"] = resolved
    return result


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
_V3_OPTIONAL: frozenset[str] = frozenset({"instruments_ref", "strategy_init_kwargs"})


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

    v3 を渡す場合は resolve_refs 後の dict（instruments キー必須）を渡すこと。
    """
    if not isinstance(d, dict):
        raise ScenarioValidationError(f"SCENARIO must be a dict, got {type(d).__name__}")

    sv = d.get("schema_version")
    # Normalize singular "instrument" key to "instruments" for v2/v3 files that use the old key.
    if sv in (2, 3) and "instrument" in d and "instruments" not in d:
        d = dict(d)
        d["instruments"] = d.pop("instrument")
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
        if "instruments_ref" in d and not isinstance(d["instruments_ref"], str):
            raise ScenarioValidationError(
                f"SCENARIO['instruments_ref'] must be str, "
                f"got {type(d['instruments_ref']).__name__}"
            )
    else:
        raise ScenarioValidationError(
            f"SCENARIO schema_version must be 1, 2 or 3, got {sv!r}"
        )
