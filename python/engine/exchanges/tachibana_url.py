import json
from typing import Literal

BASE_URL_DEMO = "https://demo-kabuka.e-shiten.jp/e_api_v4r8/"
BASE_URL_PROD = "https://kabuka.e-shiten.jp/e_api_v4r8/"

from ._env_guard import require_prod_env

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

_URLENCODE_TRANS = str.maketrans(_URLENCODE_TABLE)


def base_url(environment: Literal["demo", "prod"]) -> str:
    if environment == "demo":
        return BASE_URL_DEMO
    if environment == "prod":
        require_prod_env("TACHIBANA_ALLOW_PROD")
        return BASE_URL_PROD
    raise ValueError("invalid environment")


def func_replace_urlecnode(s: str) -> str:
    return s.translate(_URLENCODE_TRANS)


def build_request_url(base: str, json_obj: dict | str) -> str:
    if isinstance(json_obj, dict):
        payload = json.dumps(json_obj, ensure_ascii=False)
    else:
        payload = json_obj
    return f"{base}?{func_replace_urlecnode(payload)}"


def build_event_url(base: str, params: dict[str, str]) -> str:
    encoded = "&".join(f"{k}={func_replace_urlecnode(v)}" for k, v in params.items())
    return f"{base}?{encoded}"
