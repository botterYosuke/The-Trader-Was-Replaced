"""Tests for python/engine/live/logging.py mask_secrets()."""

from __future__ import annotations

from engine.live.logging import mask_secrets


def test_masks_top_level_password_and_token() -> None:
    src = {"password": "hunter2", "token": "abc.def", "user": "alice"}
    out = mask_secrets(src)
    assert out == {"password": "***", "token": "***", "user": "alice"}
    assert src == {"password": "hunter2", "token": "abc.def", "user": "alice"}


def test_leaves_non_secret_keys_untouched() -> None:
    src = {"user": "alice", "count": 3, "ratio": 0.5, "flag": True, "x": None}
    assert mask_secrets(src) == src


def test_masks_nested_dict_recursively() -> None:
    src = {
        "outer": {
            "inner": {
                "p_pwd": "secret",
                "api_key": "k-1",
                "label": "keep",
            },
            "label": "keep2",
        }
    }
    out = mask_secrets(src)
    assert out["outer"]["inner"]["p_pwd"] == "***"
    assert out["outer"]["inner"]["api_key"] == "***"
    assert out["outer"]["inner"]["label"] == "keep"
    assert out["outer"]["label"] == "keep2"


def test_masks_list_of_dicts() -> None:
    src = {"items": [{"sPassword": "a"}, {"sSecondPassword": "b"}, {"name": "c"}]}
    out = mask_secrets(src)
    assert out["items"][0] == {"sPassword": "***"}
    assert out["items"][1] == {"sSecondPassword": "***"}
    assert out["items"][2] == {"name": "c"}


def test_masks_tachibana_virtual_urls() -> None:
    src = {
        "sUrlRequest": "https://...",
        "sUrlMaster": "https://...",
        "sUrlPrice": "https://...",
        "sUrlEvent": "https://...",
        "sUrlEventWebSocket": "wss://...",
        "sUrlLower": "https://...",
        "sUrllower": "https://keep",
        "virtual_url": "https://...",
    }
    out = mask_secrets(src)
    assert out["sUrlRequest"] == "***"
    assert out["sUrlMaster"] == "***"
    assert out["sUrlPrice"] == "***"
    assert out["sUrlEvent"] == "***"
    assert out["sUrlEventWebSocket"] == "***"
    assert out["virtual_url"] == "***"
    assert out["sUrlLower"] == "***"
    assert out["sUrllower"] == "https://keep"


def test_does_not_mutate_nested_source() -> None:
    src = {"outer": {"password": "p"}}
    snapshot = {"outer": {"password": "p"}}
    _ = mask_secrets(src)
    assert src == snapshot


# --- Post-merge fix (MEDIUM-3): extended secret keyword coverage --------------

def test_masks_cookie_and_set_cookie_headers() -> None:
    src = {"cookie": "sessionid=abc123", "set-cookie": "auth=xyz", "user": "alice"}
    out = mask_secrets(src)
    assert out["cookie"] == "***"
    assert out["set-cookie"] == "***"
    assert out["user"] == "alice"


def test_masks_authorization_and_bearer() -> None:
    src = {"authorization": "Bearer abc.def", "Bearer": "tok", "user": "alice"}
    out = mask_secrets(src)
    assert out["authorization"] == "***"
    assert out["Bearer"] == "***"
    assert out["user"] == "alice"


def test_masks_apikey_case_variants() -> None:
    src = {
        "apiKey": "sk-1",
        "APIKey": "sk-2",
        "API_KEY": "sk-3",
        "api-key": "sk-4",
        "api_key": "sk-5",
        "ApIKeY": "sk-6",
        "user": "alice",
    }
    out = mask_secrets(src)
    assert out["apiKey"] == "***"
    assert out["APIKey"] == "***"
    assert out["API_KEY"] == "***"
    assert out["api-key"] == "***"
    assert out["api_key"] == "***"
    assert out["ApIKeY"] == "***"
    assert out["user"] == "alice"


def test_masks_second_password_variants() -> None:
    src = {
        "SECOND_PASSWORD": "x",
        "second_password": "y",
        "second-password": "z",
        "sSecondPassword": "w",
        "user": "alice",
    }
    out = mask_secrets(src)
    assert out["SECOND_PASSWORD"] == "***"
    assert out["second_password"] == "***"
    assert out["second-password"] == "***"
    assert out["sSecondPassword"] == "***"
    assert out["user"] == "alice"
