"""Tachibana login form state — pure presenter logic, no tkinter dependency."""
from __future__ import annotations
import os
from dataclasses import dataclass
from typing import Optional


@dataclass(frozen=True)
class FormInit:
    env_hint: str
    allow_prod: bool
    is_debug_build: bool
    dev_user_id: Optional[str]
    dev_password: Optional[str]
    dev_demo: Optional[bool]
    initial_mode: str  # "demo" or "prod"


AUTH_FAILED = "AUTH_FAILED"
NETWORK_ERROR = "NETWORK_ERROR"
SERVICE_OUT_OF_HOURS = "SERVICE_OUT_OF_HOURS"
USER_CANCELLED = "USER_CANCELLED"
EMPTY_FIELDS = "EMPTY_FIELDS"


def build_form_init(
    env_hint: str,
    env_dict: Optional[dict] = None,
    is_debug_build: bool = True,
) -> FormInit:
    if env_dict is None:
        env_dict = dict(os.environ)

    allow_prod = env_dict.get("TACHIBANA_ALLOW_PROD") == "1"

    if is_debug_build:
        dev_user_id = env_dict.get("DEV_TACHIBANA_USER_ID") or None
        dev_password = env_dict.get("DEV_TACHIBANA_PASSWORD") or None
        dev_demo_str = env_dict.get("DEV_TACHIBANA_DEMO", "true").lower()
        dev_demo: Optional[bool] = dev_demo_str not in ("false", "0", "no", "off", "")
    else:
        dev_user_id = None
        dev_password = None
        dev_demo = None

    # env_hint が "prod" かつ allow_prod のとき prod、それ以外は demo
    if env_hint == "prod" and allow_prod:
        initial_mode = "prod"
    else:
        initial_mode = "demo"

    return FormInit(
        env_hint=env_hint,
        allow_prod=allow_prod,
        is_debug_build=is_debug_build,
        dev_user_id=dev_user_id,
        dev_password=dev_password,
        dev_demo=dev_demo,
        initial_mode=initial_mode,
    )


def validate_submission(user_id: str, password: str, mode: str) -> Optional[str]:
    """Return error code string if invalid, None if OK."""
    if not user_id.strip() or not password.strip():
        return EMPTY_FIELDS
    return None
