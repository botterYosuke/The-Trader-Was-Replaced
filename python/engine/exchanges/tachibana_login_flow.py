"""Tachibana login tkinter dialog (Phase 8 §3.2.1 Step 2).

Subprocess entry point: login_dialog_runner invokes run_dialog() after
try_create_tk() returns True. Returns a dict that is emitted as NDJSON
on stdout. No secrets are emitted; on success the session URLs are
persisted via tachibana_file_store.save_session() and the cred-path is
unused on the Tachibana path.
"""
from __future__ import annotations

import asyncio
import logging
import threading
from datetime import datetime
from typing import Any, Optional
from zoneinfo import ZoneInfo

from engine.exchanges.tachibana_login_form_state import (
    AUTH_FAILED,
    NETWORK_ERROR,
    SERVICE_OUT_OF_HOURS,
    USER_CANCELLED,
    build_form_init,
    validate_submission,
)
from engine.live._build_mode import IS_DEBUG_BUILD

log = logging.getLogger(__name__)


def _map_exception(exc: BaseException) -> str:
    """Map tachibana_auth exceptions to subprocess NDJSON error_code strings."""
    # Lazy imports so module import does not require httpx/tachibana_auth.
    try:
        from engine.exchanges.tachibana_auth import (
            ApiError,
            LoginError,
            SessionExpiredError,
            UnreadNoticesError,
        )
    except Exception:  # pragma: no cover - defensive
        return AUTH_FAILED
    import httpx as _httpx

    if isinstance(exc, SessionExpiredError):
        return AUTH_FAILED
    if isinstance(exc, UnreadNoticesError):
        return AUTH_FAILED
    if isinstance(exc, ApiError) and getattr(exc, "code", None) == "-62":
        return SERVICE_OUT_OF_HOURS
    if isinstance(exc, LoginError):
        code = getattr(exc, "code", "") or ""
        if code in ("-62",):
            return SERVICE_OUT_OF_HOURS
        if code in ("transport_error",):
            return NETWORK_ERROR
        return AUTH_FAILED
    if isinstance(exc, (_httpx.ConnectError, _httpx.ReadError, _httpx.TimeoutException)):
        return NETWORK_ERROR
    return AUTH_FAILED


def run_dialog(env_hint: str) -> dict:
    """Open the Tachibana login dialog and return {"success", "error_code"}."""
    import tkinter as tk

    init = build_form_init(env_hint=env_hint, is_debug_build=IS_DEBUG_BUILD)
    if env_hint == "prod" and not init.allow_prod:
        # Defensive: login_dialog_runner front-stops this, but if a caller
        # bypasses the gate we still refuse without prompting the user.
        return {"success": False, "error_code": "PROD_NOT_ALLOWED"}

    from engine.exchanges.tachibana_auth import PNoCounter

    result: dict[str, Any] = {"success": False, "error_code": USER_CANCELLED}
    p_no_counter = PNoCounter()

    root = tk.Tk()
    root.title("Tachibana ログイン")
    root.resizable(False, False)

    user_var = tk.StringVar(value=init.dev_user_id or "")
    pw_var = tk.StringVar(value=init.dev_password or "")
    mode_var = tk.StringVar(value=init.initial_mode)
    status_var = tk.StringVar(value="")

    frm = tk.Frame(root, padx=12, pady=10)
    frm.grid(row=0, column=0)

    tk.Label(frm, text="ユーザー ID").grid(row=0, column=0, sticky="w")
    user_entry = tk.Entry(frm, textvariable=user_var, width=24)
    user_entry.grid(row=0, column=1, pady=2)

    tk.Label(frm, text="パスワード").grid(row=1, column=0, sticky="w")
    pw_entry = tk.Entry(frm, textvariable=pw_var, show="*", width=24)
    pw_entry.grid(row=1, column=1, pady=2)

    demo_radio = tk.Radiobutton(frm, text="Demo", variable=mode_var, value="demo")
    prod_radio = tk.Radiobutton(frm, text="Prod", variable=mode_var, value="prod")
    demo_radio.grid(row=2, column=0, sticky="w")
    prod_radio.grid(row=2, column=1, sticky="w")
    if not init.allow_prod:
        prod_radio.config(state="disabled")

    status_lbl = tk.Label(frm, textvariable=status_var, fg="#a00")
    status_lbl.grid(row=3, column=0, columnspan=2, sticky="w", pady=(4, 0))

    btn_frm = tk.Frame(frm)
    btn_frm.grid(row=4, column=0, columnspan=2, pady=(8, 0))
    ok_btn = tk.Button(btn_frm, text="OK", width=10)
    cancel_btn = tk.Button(btn_frm, text="Cancel", width=10)
    ok_btn.grid(row=0, column=0, padx=4)
    cancel_btn.grid(row=0, column=1, padx=4)

    def _set_busy(busy: bool) -> None:
        state = "disabled" if busy else "normal"
        ok_btn.config(state=state)
        cancel_btn.config(state=state)
        user_entry.config(state=state)
        pw_entry.config(state=state)
        demo_radio.config(state=state)
        if init.allow_prod:
            prod_radio.config(state=state)

    def _on_cancel(*_args: Any) -> None:
        result["success"] = False
        result["error_code"] = USER_CANCELLED
        root.destroy()

    def _on_auth_done(payload: tuple[str, Any]) -> None:
        kind, value = payload
        if kind == "ok":
            session = value
            try:
                from engine.exchanges.tachibana_file_store import save_session
                save_session(
                    {
                        "url_request": str(session.url_request),
                        "url_master": str(session.url_master),
                        "url_price": str(session.url_price),
                        "url_event": str(session.url_event),
                        "url_event_ws": session.url_event_ws,
                        "zyoutoeki_kazei_c": session.zyoutoeki_kazei_c,
                        "last_p_no": p_no_counter.peek(),
                        "issued_jst_date": datetime.now(
                            ZoneInfo("Asia/Tokyo")
                        ).date().isoformat(),
                    }
                )
            except Exception:
                log.exception("tachibana_login_flow: save_session failed")
                result["success"] = False
                result["error_code"] = AUTH_FAILED
                root.destroy()
                return
            result["success"] = True
            result["error_code"] = ""
            root.destroy()
        else:
            exc = value
            log.error("tachibana login failed: %r", exc)
            result["success"] = False
            result["error_code"] = _map_exception(exc)
            status_var.set(f"ログイン失敗: {result['error_code']}")
            _set_busy(False)

    def _on_ok(*_args: Any) -> None:
        user_id = user_var.get().strip()
        password = pw_var.get()
        mode = mode_var.get()
        err = validate_submission(user_id, password, mode)
        if err:
            status_var.set("ID/パスワードを入力してください")
            return
        if mode == "prod" and not init.allow_prod:
            status_var.set("Prod は TACHIBANA_ALLOW_PROD=1 が必要")
            return

        status_var.set("Authenticating...")
        _set_busy(True)

        def _run_auth() -> None:
            from engine.exchanges.tachibana_auth import login as _auth_login
            try:
                session = asyncio.run(
                    _auth_login(
                        user_id,
                        password,
                        is_demo=(mode == "demo"),
                        p_no_counter=p_no_counter,
                    )
                )
                root.after(0, _on_auth_done, ("ok", session))
            except BaseException as exc:  # noqa: BLE001 - propagate via callback
                root.after(0, _on_auth_done, ("err", exc))

        threading.Thread(target=_run_auth, daemon=True).start()

    ok_btn.config(command=_on_ok)
    cancel_btn.config(command=_on_cancel)
    root.protocol("WM_DELETE_WINDOW", _on_cancel)
    root.bind("<Return>", _on_ok)
    root.bind("<Escape>", _on_cancel)

    if init.is_debug_build and user_var.get():
        pw_entry.focus_set()
    else:
        user_entry.focus_set()

    root.mainloop()
    return {"success": bool(result["success"]), "error_code": str(result["error_code"])}
