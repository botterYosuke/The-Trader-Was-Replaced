"""kabuStation login form state — pure presenter logic, no tkinter dependency."""
from __future__ import annotations
import os
import socket
from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class FormInit:
    env_hint: str
    allow_prod: bool
    is_debug_build: bool
    dev_api_password: Optional[str]
    station_port: int


KABU_STATION_NOT_RUNNING = "KABU_STATION_NOT_RUNNING"
KABU_API_DISABLED = "KABU_API_DISABLED"
KABU_TOKEN_EXPIRED = "KABU_TOKEN_EXPIRED"
AUTH_FAILED = "AUTH_FAILED"
USER_CANCELLED = "USER_CANCELLED"
EMPTY_FIELDS = "EMPTY_FIELDS"


def build_form_init(
    env_hint: str,
    env_dict: Optional[dict] = None,
    is_debug_build: bool = True,
) -> FormInit:
    if env_dict is None:
        env_dict = dict(os.environ)

    allow_prod = env_dict.get("KABU_ALLOW_PROD") == "1"

    if is_debug_build:
        dev_api_password = env_dict.get("DEV_KABU_API_PASSWORD") or None
    else:
        dev_api_password = None

    # ポート: verify=18081, prod=18080
    if env_hint == "prod" and allow_prod:
        station_port = 18080
    else:
        station_port = 18081

    return FormInit(
        env_hint=env_hint,
        allow_prod=allow_prod,
        is_debug_build=is_debug_build,
        dev_api_password=dev_api_password,
        station_port=station_port,
    )


def probe_station(host: str = "127.0.0.1", port: int = 18081) -> bool:
    """Return True if kabuStation is listening on the given port."""
    try:
        with socket.create_connection((host, port), timeout=0.5):
            return True
    except (ConnectionRefusedError, OSError, TimeoutError):
        return False


def validate_submission(api_password: str) -> Optional[str]:
    if not api_password.strip():
        return EMPTY_FIELDS
    return None


@dataclass(frozen=True)
class AuthFailureView:
    """auth 失敗時の UI 提示。allow_retry=False なら OK を disabled のまま据え置き、
    recheck_btn で本体設定変更後の再確認を促す。"""
    status_text: str
    allow_retry: bool


def auth_failure_view(error_code: str) -> AuthFailureView:
    if error_code == KABU_API_DISABLED:
        return AuthFailureView(
            status_text="kabuStation 本体の API 設定を有効化し『再確認』を押してください",
            allow_retry=False,
        )
    if error_code == KABU_TOKEN_EXPIRED:
        return AuthFailureView(
            status_text="トークン期限切れ。API パスワードを確認して再試行してください",
            allow_retry=True,
        )
    if error_code == KABU_STATION_NOT_RUNNING:
        return AuthFailureView(
            status_text=f"{KABU_STATION_NOT_RUNNING}: kabuStation を起動し『再確認』を押してください",
            allow_retry=False,
        )
    return AuthFailureView(
        status_text=f"ログイン失敗: {error_code}",
        allow_retry=True,
    )
