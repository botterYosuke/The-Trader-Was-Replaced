"""Tachibana e-shiten REQUEST/EVENT URL builders.

This module is the **single source** for Tachibana URL construction. Standard
library URL encoders (`urllib.parse.quote`, `urlencode`, `httpx.URL(...)`'s
auto-query-encoding) MUST NOT be used: Tachibana mandates the bespoke 30-ish
character replacement table from SKILL.md R9 (`func_replace_urlecnode`), and
delegating to standard encoders breaks the contract.

NewType-style wrappers tag each virtual URL by purpose so that builders can
refuse the wrong endpoint at function boundaries (MEDIUM-C4):

* `RequestUrl` / `MasterUrl` / `PriceUrl`  — accepted by `build_request_url`
* `EventUrl`                               — accepted by `build_event_url`
* `AuthUrl`                                — accepted by `build_auth_url`

The HTTP/REST scheme of the four virtual URLs is `https://`; only
`sUrlEventWebSocket` is `wss://` (validated in `tachibana_auth`, not here).

Prod guard (TACHIBANA_ALLOW_PROD) is intentionally OUT-OF-SCOPE for A1.1; it
will land in a later step alongside the auth flow.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Mapping


@dataclass(frozen=True, slots=True)
class _BaseUrl:
    value: str

    def __str__(self) -> str:
        return self.value


class RequestUrl(_BaseUrl):
    """`sUrlRequest` — business REQUEST endpoint."""


class MasterUrl(_BaseUrl):
    """`sUrlMaster` — master data REQUEST endpoint."""


class PriceUrl(_BaseUrl):
    """`sUrlPrice` — quote-snapshot REQUEST endpoint."""


class EventUrl(_BaseUrl):
    """`sUrlEvent` / `sUrlEventWebSocket` — EVENT push endpoint."""


class AuthUrl(_BaseUrl):
    """`{BASE_URL}` — pre-login auth endpoint base."""


BASE_URL_PROD: AuthUrl = AuthUrl("https://kabuka.e-shiten.jp/e_api_v4r8/")
BASE_URL_DEMO: AuthUrl = AuthUrl("https://demo-kabuka.e-shiten.jp/e_api_v4r8/")


MASTER_CLMIDS: frozenset[str] = frozenset({
    "CLMEventDownload",
    "CLMMfdsGetMasterData",
    "CLMMfdsGetIssueDetail",
    "CLMMfdsGetNewsHead",
    "CLMMfdsGetNewsBody",
    "CLMMfdsGetSyoukinZan",
    "CLMMfdsGetShinyouZan",
    "CLMMfdsGetHibuInfo",
    "CLMIssueMstKabu",
    "CLMIssueSizyouMstKabu",
    "CLMIssueMstSak",
    "CLMIssueMstOp",
    "CLMIssueMstOther",
    "CLMOrderErrReason",
    "CLMDateZyouhou",
    "CLMYobine",
})


PRICE_CLMIDS: frozenset[str] = frozenset({
    "CLMMfdsGetMarketPrice",
    "CLMMfdsGetMarketPriceHistory",
})


_ALLOWED_OFMT = frozenset({"4", "5"})

_FORBIDDEN_CONTROL_CHARS = frozenset(chr(c) for c in range(0x20))


_REPLACE_TABLE: dict[str, str] = {
    " ": "%20",
    "!": "%21",
    '"': "%22",
    "#": "%23",
    "$": "%24",
    "%": "%25",
    "&": "%26",
    "'": "%27",
    "(": "%28",
    ")": "%29",
    "*": "%2A",
    "+": "%2B",
    ",": "%2C",
    "/": "%2F",
    ":": "%3A",
    ";": "%3B",
    "<": "%3C",
    "=": "%3D",
    ">": "%3E",
    "?": "%3F",
    "@": "%40",
    "[": "%5B",
    "]": "%5D",
    "^": "%5E",
    "`": "%60",
    "{": "%7B",
    "|": "%7C",
    "}": "%7D",
    "~": "%7E",
}


def func_replace_urlecnode(s: str) -> str:
    """Apply the Tachibana percent-encoding table to `s`."""
    return "".join(_REPLACE_TABLE.get(ch, ch) for ch in s)


def _check_no_control_chars(values: list[str]) -> None:
    for v in values:
        for ch in v:
            if ch in _FORBIDDEN_CONTROL_CHARS:
                raise ValueError(
                    f"control character {ch!r} is forbidden inside Tachibana "
                    "URLs (SKILL.md EVENT 規約 / F-M6b)"
                )


def build_request_url(
    base: RequestUrl | MasterUrl | PriceUrl,
    json_obj: Mapping[str, object],
    *,
    sJsonOfmt: str,
) -> str:
    """Build a REQUEST URL: ``{base}?{percent-encoded JSON}``."""
    if not isinstance(base, (RequestUrl, MasterUrl, PriceUrl)):
        raise TypeError(
            f"build_request_url expects RequestUrl/MasterUrl/PriceUrl, got {type(base).__name__}"
        )
    if sJsonOfmt not in _ALLOWED_OFMT:
        raise ValueError(
            f"sJsonOfmt must be '4' or '5' (R5), got {sJsonOfmt!r}"
        )

    sclmid = json_obj.get("sCLMID") if isinstance(json_obj, Mapping) else None
    if isinstance(sclmid, str):
        if sclmid in MASTER_CLMIDS and not isinstance(base, MasterUrl):
            raise TypeError(
                f"build_request_url: sCLMID={sclmid!r} requires MasterUrl, "
                f"got {type(base).__name__}"
            )
        if sclmid in PRICE_CLMIDS and not isinstance(base, PriceUrl):
            raise TypeError(
                f"build_request_url: sCLMID={sclmid!r} requires PriceUrl, "
                f"got {type(base).__name__}"
            )

    payload: dict[str, object] = {**dict(json_obj), "sJsonOfmt": sJsonOfmt}

    for key, value in payload.items():
        if not isinstance(value, (str, int, float)):
            raise TypeError(
                f"build_request_url: value for {key!r} must be str/int/float "
                f"(got {type(value).__name__}); nested types are not supported"
            )
    string_values = [str(v) for v in payload.values()]
    string_values += [str(k) for k in payload.keys()]
    _check_no_control_chars(string_values)

    serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    return f"{base.value}?{func_replace_urlecnode(serialized)}"


def build_auth_url(
    base: AuthUrl,
    json_obj: Mapping[str, object],
    *,
    sJsonOfmt: str = "5",
) -> str:
    """Build the pre-login auth URL: ``{base}auth/?{percent-encoded JSON}``."""
    if not isinstance(base, AuthUrl):
        raise TypeError(
            f"build_auth_url expects AuthUrl, got {type(base).__name__}"
        )
    if sJsonOfmt != "5":
        raise ValueError(
            f"auth endpoint requires sJsonOfmt='5' (R5), got {sJsonOfmt!r}"
        )

    payload: dict[str, object] = {**dict(json_obj), "sJsonOfmt": sJsonOfmt}

    for key, value in payload.items():
        if not isinstance(value, (str, int, float)):
            raise TypeError(
                f"build_auth_url: value for {key!r} must be str/int/float "
                f"(got {type(value).__name__}); nested types are not supported"
            )
    string_values = [str(v) for v in payload.values()]
    string_values += [str(k) for k in payload.keys()]
    _check_no_control_chars(string_values)

    serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    return f"{base.value}auth/?{func_replace_urlecnode(serialized)}"


def build_event_url(base: EventUrl, params: Mapping[str, str]) -> str:
    """Build an EVENT URL: ``{base}?key=value&key=value``."""
    if not isinstance(base, EventUrl):
        raise TypeError(
            f"build_event_url expects EventUrl, got {type(base).__name__}"
        )

    keys = [str(k) for k in params.keys()]
    values = [str(v) for v in params.values()]
    _check_no_control_chars(keys + values)

    parts = [f"{func_replace_urlecnode(k)}={func_replace_urlecnode(v)}"
             for k, v in zip(keys, values)]
    return f"{base.value}?{'&'.join(parts)}"
