def test_run_dialog_passes_is_debug_build_to_form_init(monkeypatch):
    """Release build (IS_DEBUG_BUILD=False) prevents dev prefill."""
    captured: dict = {}

    def _fake_build(**kwargs):
        captured.update(kwargs)
        raise RuntimeError("stop before tk.Tk()")

    monkeypatch.setattr(
        "engine.exchanges.kabusapi_login_flow.build_form_init", _fake_build
    )
    monkeypatch.setattr("engine.exchanges.kabusapi_login_flow.IS_DEBUG_BUILD", False)
    from engine.exchanges import kabusapi_login_flow
    try:
        kabusapi_login_flow.run_dialog(env_hint="verify", cred_path="dummy")
    except RuntimeError:
        pass
    assert captured.get("is_debug_build") is False
    assert captured.get("env_hint") == "verify"
