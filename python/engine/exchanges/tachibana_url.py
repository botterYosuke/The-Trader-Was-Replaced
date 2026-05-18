import json
import os
from typing import Literal

BASE_URL_DEMO = "https://demo-kabuka.e-shiten.jp/e_api_v4r8/"
BASE_URL_PROD = "https://kabuka.e-shiten.jp/e_api_v4r8/"

_URLENCODE_TABLE: dict[str, str] = {
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
    "\\": "%5C",
    "]": "%5D",
    "^": "%5E",
    "`": "%60",
    "{": "%7B",
    "|": "%7C",
    "}": "%7D",
    "~": "%7E",
}


def base_url(environment: Literal["demo", "prod"]) -> str:
    if environment == "demo":
        return BASE_URL_DEMO
    if environment == "prod":
        if os.environ.get("TACHIBANA_ALLOW_PROD") == "1":
            return BASE_URL_PROD
        raise RuntimeError("TACHIBANA_ALLOW_PROD env required for production")
    raise ValueError("invalid environment")


def func_replace_urlecnode(s: str) -> str:
    parts: list[str] = []
    for ch in s:
        parts.append(_URLENCODE_TABLE.get(ch, ch))
    return "".join(parts)


def build_request_url(base: str, json_obj: dict | str) -> str:
    if isinstance(json_obj, dict):
        payload = json.dumps(json_obj, ensure_ascii=False)
    else:
        payload = json_obj
    return f"{base}?{func_replace_urlecnode(payload)}"


def build_event_url(base: str, params: dict[str, str]) -> str:
    encoded = "&".join(f"{k}={func_replace_urlecnode(v)}" for k, v in params.items())
    return f"{base}?{encoded}"
