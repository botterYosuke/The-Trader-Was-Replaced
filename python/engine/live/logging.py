"""Secrets masking helper for live venue logging.

Phase 8 §3.2 / §6 で要求される、平文資格情報 / 仮想 URL を
ログに出さないための pure 関数。`logger.extra` に渡す dict や、
DEBUG ログに乗せる任意の payload を mask_secrets() に通してから
出力する想定。

対象 key (case-sensitive, 部分一致):
  password / token / api_key / p_pwd / sPassword /
  sSecondPassword / virtual_url / sUrl[A-Z]

仮想 URL (sUrlRequest / sUrlMaster / sUrlPrice / sUrlEvent /
sUrlEventWebSocket) も Tachibana ではセッション秘密なので
マスク対象。
"""

from __future__ import annotations

import re
from typing import Any

_SECRET_KEY_RE = re.compile(
    r"password|token|api_key|p_pwd|sPassword|sSecondPassword|virtual_url|sUrl[A-Z]"
)

_MASK = "***"


def _is_secret_key(key: Any) -> bool:
    if not isinstance(key, str):
        return False
    return _SECRET_KEY_RE.search(key) is not None


def mask_secrets(payload: Any) -> Any:
    """Return a deep copy of payload with secret values replaced by '***'.

    - dict: 各 key を判定し、対象なら値を '***'、それ以外は再帰
    - list / tuple: 要素ごとに再帰（tuple は tuple で返す）
    - その他: そのまま返す（immutable / scalar 想定）

    元の payload は変更しない。
    """
    if isinstance(payload, dict):
        out: dict[Any, Any] = {}
        for k, v in payload.items():
            if _is_secret_key(k):
                out[k] = _MASK
            else:
                out[k] = mask_secrets(v)
        return out
    if isinstance(payload, list):
        return [mask_secrets(item) for item in payload]
    if isinstance(payload, tuple):
        return tuple(mask_secrets(item) for item in payload)
    return payload
