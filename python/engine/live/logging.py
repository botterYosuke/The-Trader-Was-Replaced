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

import re
from typing import Any

# Case-insensitive: apiKey / APIKEY / Authorization / Bearer などを 1 つの
# regex でカバーする。sUrl[A-Z] だけは小文字 sUrlfoo が秘密じゃないので、
# 後段で個別に case-sensitive チェックする。
_SECRET_KEY_RE = re.compile(
    r"password"
    r"|token"
    r"|api[_-]?key"
    r"|p_pwd"
    r"|second[_-]?password"
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
