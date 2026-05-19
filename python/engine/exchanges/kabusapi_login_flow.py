"""kabuStation login tkinter dialog (Phase 8 §3.2.1 Step 3).

Subprocess entry point: login_dialog_runner invokes run_dialog() after
try_create_tk() returns True. The bearer token is written to ``cred_path``
(O_WRONLY|O_TRUNC) as JSON {"token": "..."} on success; never to stdout.
"""
from __future__ import annotations

import asyncio
import json
import logging
import os
import threading
from typing import Any, Optional

from engine.exchanges.kabusapi_login_form_state import (
    AUTH_FAILED,
    KABU_API_DISABLED,
    KABU_STATION_NOT_RUNNING,
    KABU_TOKEN_EXPIRED,
    USER_CANCELLED,
    auth_failure_view,
    build_form_init,
    probe_station,
    validate_submission,
)
from engine.live._build_mode import IS_DEBUG_BUILD

log = logging.getLogger(__name__)


def _map_exception(exc: BaseException) -> str:
    try:
        from engine.exchanges.kabusapi_auth import (
            KabuApiError,
            KabuConnectionError,
            KabuTokenExpiredError,
        )
    except Exception:  # pragma: no cover
        return AUTH_FAILED

    if isinstance(exc, KabuTokenExpiredError):
        return KABU_TOKEN_EXPIRED
    if isinstance(exc, KabuConnectionError):
        return KABU_STATION_NOT_RUNNING
    if isinstance(exc, KabuApiError):
        code = getattr(exc, "code", None)
        if code in (4001003, "4001003"):
            return KABU_API_DISABLED
        if code in (4001005, "4001005"):
            return KABU_TOKEN_EXPIRED
        return AUTH_FAILED
    return AUTH_FAILED


def run_dialog(env_hint: str, cred_path: str) -> dict:
    """Open the kabuStation login dialog and return {"success", "error_code"}."""
    import tkinter as tk

    init = build_form_init(env_hint=env_hint, is_debug_build=IS_DEBUG_BUILD)
    if env_hint == "prod" and not init.allow_prod:
        return {"success": False, "error_code": "PROD_NOT_ALLOWED"}

    result: dict[str, Any] = {"success": False, "error_code": USER_CANCELLED}

    root = tk.Tk()
    root.title("kabuStation ログイン")
    root.resizable(False, False)

    pw_var = tk.StringVar(value=init.dev_api_password or "")
    mode_var = tk.StringVar(value="prod" if (env_hint == "prod" and init.allow_prod) else "verify")
    status_var = tk.StringVar(value="")
    port_var = tk.StringVar(value=str(init.station_port))

    frm = tk.Frame(root, padx=12, pady=10)
    frm.grid(row=0, column=0)

    tk.Label(frm, text="API パスワード").grid(row=0, column=0, sticky="w")
    pw_entry = tk.Entry(frm, textvariable=pw_var, show="*", width=24)
    pw_entry.grid(row=0, column=1, pady=2)

    verify_radio = tk.Radiobutton(frm, text="Verify", variable=mode_var, value="verify")
    prod_radio = tk.Radiobutton(frm, text="Prod", variable=mode_var, value="prod")
    verify_radio.grid(row=1, column=0, sticky="w")
    prod_radio.grid(row=1, column=1, sticky="w")
    if not init.allow_prod:
        prod_radio.config(state="disabled")

    tk.Label(frm, text="本体ポート:").grid(row=2, column=0, sticky="w")
    tk.Label(frm, textvariable=port_var).grid(row=2, column=1, sticky="w")

    status_lbl = tk.Label(frm, textvariable=status_var, fg="#a00")
    status_lbl.grid(row=3, column=0, columnspan=3, sticky="w", pady=(4, 0))

    btn_frm = tk.Frame(frm)
    btn_frm.grid(row=4, column=0, columnspan=3, pady=(8, 0))
    ok_btn = tk.Button(btn_frm, text="OK", width=10)
    recheck_btn = tk.Button(btn_frm, text="再確認", width=10)
    cancel_btn = tk.Button(btn_frm, text="Cancel", width=10)
    ok_btn.grid(row=0, column=0, padx=4)
    recheck_btn.grid(row=0, column=1, padx=4)
    cancel_btn.grid(row=0, column=2, padx=4)

    def _current_port() -> int:
        return 18080 if (mode_var.get() == "prod" and init.allow_prod) else 18081

    def _refresh_station_status() -> None:
        port = _current_port()
        port_var.set(str(port))
        if probe_station(port=port):
            status_var.set("")
            ok_btn.config(state="normal")
        else:
            status_var.set(f"{KABU_STATION_NOT_RUNNING}: port {port}")
            ok_btn.config(state="disabled")

    def _set_busy(busy: bool, *, ok_btn_active: Optional[bool] = None) -> None:
        state = "disabled" if busy else "normal"
        ok_state = state if ok_btn_active is None else ("normal" if ok_btn_active else "disabled")
        ok_btn.config(state=ok_state)
        cancel_btn.config(state=state)
        recheck_btn.config(state=state)
        pw_entry.config(state=state)
        verify_radio.config(state=state)
        if init.allow_prod:
            prod_radio.config(state=state)
        if not busy and ok_btn_active is None:
            _refresh_station_status()

    def _on_cancel(*_args: Any) -> None:
        result["success"] = False
        result["error_code"] = USER_CANCELLED
        root.destroy()

    def _on_auth_done(payload: tuple[str, Any]) -> None:
        kind, value = payload
        if kind == "ok":
            token = value
            try:
                fd = os.open(cred_path, os.O_WRONLY | os.O_TRUNC)
                try:
                    os.write(fd, json.dumps({"token": token}).encode("utf-8"))
                finally:
                    os.close(fd)
            except OSError:
                log.exception("kabu_login_flow: cred-path write failed")
                result["success"] = False
                result["error_code"] = AUTH_FAILED
                root.destroy()
                return
            result["success"] = True
            result["error_code"] = ""
            root.destroy()
        else:
            exc = value
            log.error("kabu login failed: %r", exc)
            ec = _map_exception(exc)
            view = auth_failure_view(ec)
            result["success"] = False
            result["error_code"] = ec
            status_var.set(view.status_text)
            _set_busy(False, ok_btn_active=view.allow_retry)

    def _on_ok(*_args: Any) -> None:
        api_password = pw_var.get()
        err = validate_submission(api_password)
        if err:
            status_var.set("API パスワードを入力してください")
            return
        mode = mode_var.get()
        if mode == "prod" and not init.allow_prod:
            status_var.set("Prod は KABU_ALLOW_PROD=1 が必要")
            return
        port = _current_port()
        if not probe_station(port=port):
            status_var.set(f"{KABU_STATION_NOT_RUNNING}: port {port}")
            return

        status_var.set("Authenticating...")
        _set_busy(True)

        env_arg = "prod" if mode == "prod" else "verify"

        def _run_auth() -> None:
            from engine.exchanges.kabusapi_auth import fetch_token
            try:
                token = asyncio.run(fetch_token(api_password, env=env_arg))
                root.after(0, _on_auth_done, ("ok", token))
            except BaseException as exc:  # noqa: BLE001
                root.after(0, _on_auth_done, ("err", exc))

        threading.Thread(target=_run_auth, daemon=True).start()

    ok_btn.config(command=_on_ok)
    cancel_btn.config(command=_on_cancel)
    recheck_btn.config(command=_refresh_station_status)
    root.protocol("WM_DELETE_WINDOW", _on_cancel)
    root.bind("<Return>", _on_ok)
    root.bind("<Escape>", _on_cancel)

    _refresh_station_status()
    pw_entry.focus_set()

    root.mainloop()
    return {"success": bool(result["success"]), "error_code": str(result["error_code"])}
