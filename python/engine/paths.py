from __future__ import annotations
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def resolve_repo_relative(value) -> Path:
    path = Path(value)
    return path if path.is_absolute() else REPO_ROOT / path


def artifacts_root() -> Path:
    return resolve_repo_relative(os.environ.get("ARTIFACTS_PATH", "artifacts"))


# テスト・外部コード向けエイリアス
def artifacts_dir() -> Path:
    return artifacts_root()


def jquants_catalog_path() -> Path:
    return artifacts_root() / "jquants-catalog"


# テスト・外部コード向けエイリアス
def catalog_path() -> Path:
    return jquants_catalog_path()


def instrument_lists_dir() -> Path:
    return artifacts_root() / "instrument-lists"


def listed_symbols_artifact_path(end_date: str) -> Path:
    return instrument_lists_dir() / f"listed-symbols-{end_date}.json"


def jquants_cache_dir() -> Path | None:
    value = os.environ.get("DEV_J_QUANTS_CACHE")
    return Path(value) if value else None
