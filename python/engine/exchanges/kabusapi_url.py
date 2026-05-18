"""kabusapi URL builder (kabu skill R1 / R4)."""

from __future__ import annotations

from typing import Literal

from ._env_guard import require_prod_env

BASE_URL_PROD = "http://localhost:18080/kabusapi/"
BASE_URL_VERIFY = "http://localhost:18081/kabusapi/"

Env = Literal["prod", "verify"]


def base_url(env: Env) -> str:
    """Return base URL for given env.

    - verify: always allowed (kabu skill R1: 検証 18081 が既定).
    - prod: only when env var KABU_ALLOW_PROD == "1" (二重ガード).
    """
    if env == "verify":
        return BASE_URL_VERIFY
    if env == "prod":
        require_prod_env("KABU_ALLOW_PROD")
        return BASE_URL_PROD
    raise ValueError("invalid env")


def endpoint(path: str, *, env: Env) -> str:
    """Join base URL and path (strips leading slash on path)."""
    return f"{base_url(env)}{path.lstrip('/')}"


def symbol_key(symbol: str, exchange: int) -> str:
    """Return kabu symbol key as '<symbol>@<exchange>' (kabu skill R4).

    exchange の妥当性検証は instrument_mapping 層の責務。ここでは単純結合。
    """
    if not symbol:
        raise ValueError("INVALID_SYMBOL: empty")
    return f"{symbol}@{exchange}"
