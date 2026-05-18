"""Shared production-env guard used by venue URL builders.

Phase 8 §3.2: Tachibana / kabu はそれぞれ本番接続用の二重ガード env
(TACHIBANA_ALLOW_PROD / KABU_ALLOW_PROD) を持つ。文字列名以外は同じ判定なので
1 関数に集約し、`base_url(env="prod")` 経路の唯一の解禁点にする。
"""

from __future__ import annotations

import os


def require_prod_env(var_name: str) -> None:
    """Raise RuntimeError unless ``var_name`` env var is set to "1".

    生のリテラル "1" を venue 側に散らさないための単一の解禁点。
    """
    if os.environ.get(var_name) != "1":
        raise RuntimeError(f"{var_name} env required for production")
