"""Secrets masking helper for live venue logging.

Phase 8 §3.2 / §6 で要求される、平文資格情報 / 仮想 URL を
ログに出さないための pure 関数。`logger.extra` に渡す dict や、
DEBUG ログに乗せる任意の payload を mask_secrets() に通してから
出力する想定。

対象 key (case-insensitive, 部分一致):
  password / token / api_key / apiKey / p_pwd / sPassword /
  sSecondPassword / SECOND_PASSWORD / virtual_url / sUrl[A-Z] /
  cookie / set-cookie / authorization / bearer

仮想 URL (sUrlRequest / sUrlMaster / sUrlPrice / sUrlEvent /
sUrlEventWebSocket) も Tachibana ではセッション秘密なので
マスク対象。

Post-merge fix (MEDIUM-3): cookie / authorization / bearer / apiKey 系を追加。
HTTP ヘッダや OAuth bearer も log に混ざるとセッション乗っ取りに直結するため。
"""

from __future__ import annotations

import dataclasses
import re
from typing import Any

try:  # pydantic is a hard project dependency; guard only so logging never hard-fails.
    from pydantic import BaseModel as _PydanticBaseModel
except Exception:  # pragma: no cover - pydantic always present in this project
    _PydanticBaseModel = None  # type: ignore[assignment]

# Hard cap on recursion so a self-referential / cyclic payload (a dict that contains
# itself, or a model whose model_dump() reconstructs a cycle) can never drive
# mask_secrets into a RecursionError *at log time* (MEDIUM-1 follow-up). A masking
# helper that crashes the log call defeats its own purpose.
_MAX_MASK_DEPTH = 25

# Case-insensitive: apiKey / APIKEY / Authorization / Bearer などを 1 つの
# regex でカバーする。sUrl[A-Z] だけは小文字 sUrlfoo が秘密じゃないので、
# 後段で個別に case-sensitive チェックする。
_SECRET_KEY_RE = re.compile(
    r"password"
    r"|token"
    r"|api[_-]?key"
    r"|p_pwd"
    r"|second[_-]?password"
    # Phase 9: mask any key containing "secret" — covers the proto wire field
    # `second_secret` (PlaceOrder/Cancel/ModifyOrderReq), which `second[_-]?password`
    # missed (it has no "password" token), plus client_secret / *_secret in general.
    r"|secret"
    r"|virtual_url"
    r"|cookie"
    r"|set-cookie"
    r"|authorization"
    r"|bearer",
    re.IGNORECASE,
)

# sUrl[A-Z] は case-sensitive のままにしておく
# (sUrlRequest は秘密 / sUrllower は非秘密、というレガシー慣例)。
_SURL_RE = re.compile(r"sUrl[A-Z]")

_MASK = "***"


def _is_secret_key(key: Any) -> bool:
    if not isinstance(key, str):
        return False
    if _SECRET_KEY_RE.search(key) is not None:
        return True
    if _SURL_RE.search(key) is not None:
        return True
    return False


def mask_secrets(payload: Any) -> Any:
    """Return a deep copy of payload with secret values replaced by '***'.

    - dict: 各 key を判定し、対象なら値を '***'、それ以外は再帰
    - list / tuple: 要素ごとに再帰（tuple は tuple で返す）
    - pydantic BaseModel: model_dump() の dict を再帰
      （MEDIUM-1: VenueCredentials / OrderResult 等を直接 log すると repr に
      token / second_secret が平文で漏れるため、masked dict に変換して返す）
    - dataclass: dataclasses.asdict() の dict を再帰
    - その他: そのまま返す（immutable / scalar 想定）

    元の payload は変更しない（マスク済みの新オブジェクトを返す）。protobuf 等の
    任意オブジェクトは対象外（scalar 扱いでそのまま返す）。深さ ``_MAX_MASK_DEPTH``
    を超えると ``'<max-depth>'`` を返して再帰を打ち切る（cyclic payload 防御）。
    """
    return _mask(payload, 0)


def _mask(payload: Any, depth: int) -> Any:
    if depth >= _MAX_MASK_DEPTH:
        # Cyclic / pathologically-nested payload: bail before RecursionError. The
        # bail-out is itself non-secret (a sentinel string), so nothing leaks.
        return "<max-depth>"
    if isinstance(payload, dict):
        out: dict[Any, Any] = {}
        for k, v in payload.items():
            if _is_secret_key(k):
                out[k] = _MASK
            else:
                out[k] = _mask(v, depth + 1)
        return out
    if isinstance(payload, list):
        return [_mask(item, depth + 1) for item in payload]
    if isinstance(payload, tuple):
        return tuple(_mask(item, depth + 1) for item in payload)
    # pydantic BaseModel (v2: model_dump). dataclass の前に判定する。`model_dump` の
    # duck-type は広すぎる（任意 obj の callable を呼んでしまう）ので isinstance で絞る。
    if _PydanticBaseModel is not None:
        if isinstance(payload, _PydanticBaseModel):
            try:
                return _mask(payload.model_dump(), depth + 1)
            except Exception:  # noqa: BLE001 — defensive: 怪しい dump はそのまま返す
                return payload
    else:  # pragma: no cover - pydantic is a hard project dependency
        # Degraded env (pydantic import failed): we cannot isinstance-gate, so fall
        # back to a `model_dump` duck-type PURELY for masking. The dump output is
        # re-masked below, so this is fail-safe (prefer masking a model's fields over
        # leaking its repr); the duck-type breadth only matters in this never-reached
        # path.
        model_dump = getattr(payload, "model_dump", None)
        if callable(model_dump):
            try:
                return _mask(model_dump(), depth + 1)
            except Exception:  # noqa: BLE001
                return payload
    # plain dataclass instance (not the class itself)
    if dataclasses.is_dataclass(payload) and not isinstance(payload, type):
        try:
            return _mask(dataclasses.asdict(payload), depth + 1)
        except Exception:  # noqa: BLE001
            return payload
    return payload
