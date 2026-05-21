"""Tests for TachibanaAdapter (Phase 8 §1.3 skeleton + §3.2 A1.5 login wire-up)."""

import asyncio
import json
import re

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.tachibana import TachibanaAdapter
from engine.exchanges.tachibana_auth import TachibanaSession
from engine.exchanges.tachibana_url import BASE_URL_DEMO
from engine.live.adapter import InstrumentRaw, LiveVenueAdapter, VenueCredentials


def test_venue_id_is_tachibana():
    assert TachibanaAdapter().venue_id == "TACHIBANA"


def test_protocol_compliance():
    assert isinstance(TachibanaAdapter(), LiveVenueAdapter)


def test_default_environment_is_demo():
    assert TachibanaAdapter()._env == "demo"


def test_environment_demo_accepted():
    assert TachibanaAdapter(environment="demo")._env == "demo"


def test_environment_prod_accepted():
    assert TachibanaAdapter(environment="prod")._env == "prod"


def test_invalid_environment_raises():
    with pytest.raises(ValueError):
        TachibanaAdapter(environment="staging")  # type: ignore[arg-type]


def test_logout_clears_session():
    a = TachibanaAdapter()
    a._session = "sentinel"  # type: ignore[assignment]
    asyncio.run(a.logout())
    assert a._session is None


# ---------------------------------------------------------------------------
# Phase 8 §3.2 Step 0: is_logged_in property
# ---------------------------------------------------------------------------


def test_is_logged_in_false_when_no_session():
    adapter = TachibanaAdapter()
    assert adapter.is_logged_in is False


def test_is_logged_in_true_when_session_set():
    adapter = TachibanaAdapter()
    adapter._session = "sentinel"  # type: ignore[assignment]
    assert adapter.is_logged_in is True


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A1.5: login() wire-up
# ---------------------------------------------------------------------------

_DEMO_BASE = BASE_URL_DEMO.value
_DEMO_HOST_PATH = _DEMO_BASE.removeprefix("https://").removesuffix("/")
_AUTH_URL_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}auth/\?")


def _ok_login_payload() -> dict:
    return {
        "p_no": "1",
        "p_sd_date": "2026.04.25-10:00:00.000",
        "p_errno": "0",
        "p_err": "",
        "sCLMID": "CLMAuthLoginAck",
        "sResultCode": "0",
        "sResultText": "",
        "sZyoutoekiKazeiC": "1",
        "sKinsyouhouMidokuFlg": "0",
        "sUrlRequest": f"{_DEMO_BASE}request/ND=/",
        "sUrlMaster": f"{_DEMO_BASE}master/ND=/",
        "sUrlPrice": f"{_DEMO_BASE}price/ND=/",
        "sUrlEvent": f"{_DEMO_BASE}event/ND=/",
        "sUrlEventWebSocket": f"wss://{_DEMO_HOST_PATH}/event_ws/ND=/",
    }


def _add_login_response(httpx_mock: HTTPXMock, payload: dict) -> None:
    httpx_mock.add_response(
        url=_AUTH_URL_RE,
        method="GET",
        content=json.dumps(payload, ensure_ascii=False).encode("shift_jis"),
    )


async def test_login_session_cache_missing(monkeypatch, tmp_path):
    """load_session が None を返す → SESSION_CACHE_MISSING。"""
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(tmp_path / "no_such.json"))
    creds = VenueCredentials(credentials_source="session_cache")
    with pytest.raises(ValueError, match="SESSION_CACHE_MISSING"):
        await TachibanaAdapter().login(creds)


async def test_login_session_cache_expired(monkeypatch, tmp_path):
    """有効な dict だが is_session_valid_for_today が False → SESSION_CACHE_EXPIRED。"""
    import json as _json
    session_file = tmp_path / "session.json"
    session_file.write_text(
        _json.dumps({"issued_jst_date": "2000-01-01", "url_request": "https://x/",
                     "url_master": "https://x/", "url_price": "https://x/",
                     "url_event": "https://x/", "url_event_ws": "wss://x/"}),
        encoding="utf-8",
    )
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(session_file))
    creds = VenueCredentials(credentials_source="session_cache")
    with pytest.raises(ValueError, match="SESSION_CACHE_EXPIRED"):
        await TachibanaAdapter().login(creds)


async def test_login_session_cache_restores(monkeypatch, tmp_path):
    """save_session で書いた dict を差し込み、login 後に is_logged_in==True。
    last_p_no が現在カウンタより大きい場合はカウンタを fast-forward する。
    """
    import json as _json
    from datetime import datetime
    from zoneinfo import ZoneInfo
    today = datetime.now(ZoneInfo("Asia/Tokyo")).date().isoformat()
    # last_p_no を現実の time.time() より確実に大きな値にして fast-forward を起動する
    large_p_no = 9_999_999_999
    session_data = {
        "issued_jst_date": today,
        "url_request": "https://demo/request/",
        "url_master": "https://demo/master/",
        "url_price": "https://demo/price/",
        "url_event": "https://demo/event/",
        "url_event_ws": "wss://demo/event_ws/",
        "zyoutoeki_kazei_c": "1",
        "last_p_no": large_p_no,
    }
    session_file = tmp_path / "session.json"
    session_file.write_text(_json.dumps(session_data), encoding="utf-8")
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(session_file))

    adapter = TachibanaAdapter()
    await adapter.login(VenueCredentials(credentials_source="session_cache"))

    assert adapter.is_logged_in is True
    assert adapter._session.zyoutoeki_kazei_c == "1"
    # fast-forward: last_p_no > 初期値(time.time()ベース) なので
    # カウンタが last_p_no + 1 まで進む (clock-skew no-op を防ぐ +1 セマンティクス)。
    assert adapter._p_no_counter.peek() == large_p_no + 1
    # 次の next() は last_p_no を確実に上回る (R4: p_no 重複禁止)。
    assert adapter._p_no_counter.next() > large_p_no


async def test_login_session_cache_backward_compat_no_last_p_no(monkeypatch, tmp_path):
    """last_p_no が dict にない旧フォーマットでも正常に復元できる。"""
    import json as _json
    from datetime import datetime
    from zoneinfo import ZoneInfo
    today = datetime.now(ZoneInfo("Asia/Tokyo")).date().isoformat()
    session_data = {
        "issued_jst_date": today,
        "url_request": "https://demo/request/",
        "url_master": "https://demo/master/",
        "url_price": "https://demo/price/",
        "url_event": "https://demo/event/",
        "url_event_ws": "wss://demo/event_ws/",
        # last_p_no なし (旧フォーマット)
    }
    session_file = tmp_path / "session.json"
    session_file.write_text(_json.dumps(session_data), encoding="utf-8")
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(session_file))

    adapter = TachibanaAdapter()
    await adapter.login(VenueCredentials(credentials_source="session_cache"))

    assert adapter.is_logged_in is True
    assert adapter._session.zyoutoeki_kazei_c == ""  # default empty string


async def test_login_env_missing_user_id_raises(monkeypatch):
    monkeypatch.delenv("DEV_TACHIBANA_USER_ID", raising=False)
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError, match="DEV_TACHIBANA_USER_ID"):
        await TachibanaAdapter().login(creds)


async def test_login_env_missing_password_raises(monkeypatch):
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.delenv("DEV_TACHIBANA_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError, match="DEV_TACHIBANA_PASSWORD"):
        await TachibanaAdapter().login(creds)


async def test_login_env_does_not_leak_credentials_in_exception(monkeypatch):
    """R10 — exception message must not contain the credential values."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "secret-uid-xyz")
    monkeypatch.delenv("DEV_TACHIBANA_PASSWORD", raising=False)
    creds = VenueCredentials(credentials_source="env")
    with pytest.raises(ValueError) as exc_info:
        await TachibanaAdapter().login(creds)
    assert "secret-uid-xyz" not in str(exc_info.value)


async def test_login_env_demo_success_stores_session(
    monkeypatch, httpx_mock: HTTPXMock
):
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())

    adapter = TachibanaAdapter(environment="demo")
    await adapter.login(VenueCredentials(credentials_source="env"))

    assert isinstance(adapter._session, TachibanaSession)
    assert adapter._session.url_event_ws.startswith("wss://")


async def test_login_env_demo_uses_adapter_p_no_counter(
    monkeypatch, httpx_mock: HTTPXMock
):
    """R4 — the adapter's PNoCounter must advance across login calls."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())
    _add_login_response(httpx_mock, _ok_login_payload())

    adapter = TachibanaAdapter(environment="demo")
    before = adapter._p_no_counter.peek()
    await adapter.login(VenueCredentials(credentials_source="env"))
    after_first = adapter._p_no_counter.peek()
    await adapter.login(VenueCredentials(credentials_source="env"))
    after_second = adapter._p_no_counter.peek()

    assert after_first == before + 1
    assert after_second == after_first + 1


async def test_login_env_prod_without_allow_prod_raises(monkeypatch):
    """Production double-guard: TACHIBANA_ALLOW_PROD must be '1'."""
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    monkeypatch.delenv("TACHIBANA_ALLOW_PROD", raising=False)

    adapter = TachibanaAdapter(environment="prod")
    with pytest.raises(RuntimeError, match="TACHIBANA_ALLOW_PROD"):
        await adapter.login(VenueCredentials(credentials_source="env"))


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A2.3b: fetch_instruments() — CLMEventDownload streaming
# ---------------------------------------------------------------------------

_MASTER_URL_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}master/")


def _terminator_bytes() -> bytes:
    return json.dumps({"sCLMID": "CLMEventDownloadComplete"}, ensure_ascii=False).encode("shift_jis")


def _master_records_bytes() -> bytes:
    """Minimal CLMEventDownload stream: 1 issue + 1 sizyou + 1 yobine + terminator."""
    yobine = {"sCLMID": "CLMYobine", "sYobineTaniNumber": "Y1"}
    yobine["sKizunPrice_1"] = "3000"
    yobine["sYobineTanka_1"] = "1"
    yobine["sDecimal_1"] = "0"
    for i in range(2, 21):
        yobine[f"sKizunPrice_{i}"] = "999999999"
        yobine[f"sYobineTanka_{i}"] = "1"
        yobine[f"sDecimal_{i}"] = "0"
    recs = [
        {"sCLMID": "CLMIssueMstKabu", "sIssueCode": "7203", "sIssueName": "トヨタ自動車"},
        {"sCLMID": "CLMIssueSizyouMstKabu", "sIssueCode": "7203",
         "sSizyouC": "00", "sYobineTaniNumber": "Y1", "sBaibaiTaniNumber": "100"},
        yobine,
    ]
    body = b""
    for r in recs:
        body += json.dumps(r, ensure_ascii=False).encode("shift_jis")
    body += _terminator_bytes()
    return body


async def _login_demo(monkeypatch, httpx_mock: HTTPXMock) -> TachibanaAdapter:
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())
    adapter = TachibanaAdapter(environment="demo")
    await adapter.login(VenueCredentials(credentials_source="env"))
    return adapter


async def test_fetch_instruments_requires_login():
    """No session yet → raises (不可 to build a master URL without sUrlMaster)."""
    adapter = TachibanaAdapter(environment="demo")
    with pytest.raises(Exception):
        await adapter.fetch_instruments()


async def test_fetch_instruments_returns_instrument_raw_list(monkeypatch, httpx_mock: HTTPXMock):
    adapter = await _login_demo(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=_master_records_bytes())

    out = await adapter.fetch_instruments()
    assert isinstance(out, list)
    assert len(out) == 1
    assert isinstance(out[0], InstrumentRaw)
    assert out[0].code == "7203"
    assert out[0].name == "トヨタ自動車"
    assert out[0].market == "00"
    assert out[0].lot_size == 100
    assert out[0].tick_size == 1.0


async def test_fetch_instruments_hits_url_master_endpoint(monkeypatch, httpx_mock: HTTPXMock):
    """The master DL must be issued against sUrlMaster, not sUrlRequest/sUrlPrice."""
    adapter = await _login_demo(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=_master_records_bytes())

    await adapter.fetch_instruments()
    # 2 requests: 1 login (auth/), 1 master DL (master/)
    requests = httpx_mock.get_requests()
    master_reqs = [r for r in requests if "/master/" in str(r.url)]
    assert len(master_reqs) == 1
    assert "CLMEventDownload" in str(master_reqs[0].url)


async def test_fetch_instruments_advances_p_no_counter(monkeypatch, httpx_mock: HTTPXMock):
    adapter = await _login_demo(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=_master_records_bytes())

    before = adapter._p_no_counter.peek()
    await adapter.fetch_instruments()
    after = adapter._p_no_counter.peek()
    assert after == before + 1


async def test_fetch_instruments_handles_chunked_response(monkeypatch, httpx_mock: HTTPXMock):
    """Stream parser must reassemble records across SJIS-multibyte chunk boundaries."""
    adapter = await _login_demo(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=_master_records_bytes())

    out = await adapter.fetch_instruments()
    assert len(out) == 1
    assert out[0].name == "トヨタ自動車"  # SJIS round-trip OK


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A3.3: subscribe / events wire-up to TickerEventWsHub
# ---------------------------------------------------------------------------

async def test_subscribe_requires_login():
    """No session → subscribe must reject (sUrlEventWebSocket only comes from login)."""
    adapter = TachibanaAdapter(environment="demo")
    with pytest.raises(RuntimeError, match="login"):
        await adapter.subscribe("7203.TSE", {"trades", "depth"})


async def test_subscribe_creates_hub_for_ticker(monkeypatch, httpx_mock: HTTPXMock):
    """subscribe('7203.TSE', ...) は ticker '7203' の TickerEventWsHub を 1 本作る。"""
    adapter = await _login_demo(monkeypatch, httpx_mock)

    created: list[tuple[str, str]] = []  # (ws_url, ticker)

    from engine.exchanges import tachibana as _tach_mod

    class _StubHub:
        def __init__(self, ws_url, *, ticker, proxy=None):
            created.append((ws_url, ticker))
            self.ticker = ticker
            self._subs: dict = {}
        async def subscribe(self, key, callback, *, on_connect=None, on_close=None):
            self._subs[key] = callback
        async def unsubscribe(self, key):
            self._subs.pop(key, None)
        async def aclose(self):
            self._subs.clear()

    monkeypatch.setattr(_tach_mod, "TickerEventWsHub", _StubHub)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    assert len(created) == 1
    ws_url, ticker = created[0]
    assert ticker == "7203"
    # fix1 (A3.3 review): build_event_url で query 付与された URL が渡される。
    # prefix が url_event_ws と一致し、必須 query が含まれていれば契約 OK。
    assert ws_url.startswith(adapter._session.url_event_ws)
    assert "p_issue_code=7203" in ws_url
    assert "p_evt_cmd=" in ws_url


async def test_subscribe_same_ticker_twice_reuses_hub(monkeypatch, httpx_mock: HTTPXMock):
    """同じ instrument_id を二回 subscribe しても hub は 1 本だけ。"""
    adapter = await _login_demo(monkeypatch, httpx_mock)

    created: list = []
    from engine.exchanges import tachibana as _tach_mod

    class _StubHub:
        def __init__(self, ws_url, *, ticker, proxy=None):
            created.append(ticker)
            self._subs: dict = {}
        async def subscribe(self, key, callback, *, on_connect=None, on_close=None):
            self._subs[key] = callback
        async def unsubscribe(self, key):
            self._subs.pop(key, None)
        async def aclose(self):
            self._subs.clear()

    monkeypatch.setattr(_tach_mod, "TickerEventWsHub", _StubHub)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    assert len(created) == 1


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A3.3-3: subscribe callback wiring / unsubscribe / events()
# ---------------------------------------------------------------------------


class _RecordingStubHub:
    """A3.3-3 用 StubHub。subscribe された (key, callback) を記録し、
    外部から fake frame を発火できる test double。"""

    instances: list["_RecordingStubHub"] = []

    def __init__(self, ws_url, *, ticker, proxy=None):
        self.ws_url = ws_url
        self.ticker = ticker
        self.proxy = proxy
        self._subs: dict = {}
        self._on_connect: dict = {}
        self._on_close: dict = {}
        self.aclose_called = False
        _RecordingStubHub.instances.append(self)

    async def subscribe(self, key, callback, *, on_connect=None, on_close=None):
        self._subs[key] = callback
        if on_connect is not None:
            self._on_connect[key] = on_connect
        if on_close is not None:
            self._on_close[key] = on_close

    async def unsubscribe(self, key):
        self._subs.pop(key, None)
        self._on_connect.pop(key, None)
        self._on_close.pop(key, None)

    async def aclose(self):
        self.aclose_called = True
        self._subs.clear()

    @property
    def subscriber_count(self) -> int:
        return len(self._subs)

    async def fire(self, frame_type: str, fields: dict, recv_ts_ms: int) -> None:
        """Invoke every registered callback (simulates one EVENT frame)."""
        for cb in list(self._subs.values()):
            await cb(frame_type, fields, recv_ts_ms)


def _install_stub_hub(monkeypatch) -> type[_RecordingStubHub]:
    _RecordingStubHub.instances = []
    from engine.exchanges import tachibana as _tach_mod
    monkeypatch.setattr(_tach_mod, "TickerEventWsHub", _RecordingStubHub)
    return _RecordingStubHub


def _fd_fields_first_frame() -> dict:
    """row='1' の FD 1 件目 (DV 初期化のみ、trade は出ない)。depth 1 段だけ。"""
    return {
        "p_cmd": "FD",
        "p_1_DPP": "3000",
        "p_1_DV":  "100",
        "p_1_GBP1": "2999", "p_1_GBV1": "10",
        "p_1_GAP1": "3001", "p_1_GAV1": "20",
    }


def _fd_fields_buy_trade() -> dict:
    """row='1' の FD 2 件目 (DV 進む + price>=prev_ask で buy)。depth も同梱。"""
    return {
        "p_cmd": "FD",
        "p_1_DPP": "3001",       # >= prev_ask 3001 → buy
        "p_1_DV":  "150",        # qty = 150 - 100 = 50
        "p_1_GBP1": "3000", "p_1_GBV1": "12",
        "p_1_GAP1": "3002", "p_1_GAV1": "18",
    }


def _fd_fields_ambiguous_trade() -> dict:
    """midpoint かつ prev_trade とも同値で tick rule も中立 → side='unknown'。

    2 件目の trade で _prev_trade_price = 3001 になる前提なので、
    3 件目は DPP=3001 (= prev_trade) かつ < prev_ask=3002, > prev_bid=3000
    で完全 midpoint を作る。
    """
    return {
        "p_cmd": "FD",
        "p_1_DPP": "3001",       # == prev_trade_price (3001) → tick neutral
        "p_1_DV":  "200",        # qty = 200 - 150 = 50
        "p_1_GBP1": "3000", "p_1_GBV1": "5",   # prev_bid=3000 (3001 > 3000 → bid 不適合)
        "p_1_GAP1": "3002", "p_1_GAV1": "5",   # prev_ask=3002 (3001 < 3002 → ask 不適合)
    }


async def test_subscribe_registers_callback_on_hub(monkeypatch, httpx_mock: HTTPXMock):
    """subscribe は hub.subscribe(key, callback) を 1 回呼ぶ。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    assert len(Stub.instances) == 1
    hub = Stub.instances[0]
    assert hub.subscriber_count == 1


async def test_unsubscribe_removes_callback_and_closes_hub(
    monkeypatch, httpx_mock: HTTPXMock,
):
    """最後の subscriber が外れたら hub.aclose() が呼ばれ、self._hubs から削除される。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    hub = Stub.instances[0]
    await adapter.unsubscribe("7203.TSE")

    assert hub.subscriber_count == 0
    assert hub.aclose_called is True
    assert "7203" not in adapter._hubs


async def test_unsubscribe_unknown_instrument_is_noop(
    monkeypatch, httpx_mock: HTTPXMock,
):
    """未登録 instrument の unsubscribe は例外を出さない (no-op)。"""
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)
    # 例外が出なければ PASS
    await adapter.unsubscribe("9999.TSE")


async def test_events_yields_depth_update_from_fd_frame(
    monkeypatch, httpx_mock: HTTPXMock,
):
    """FD 1 件目 (depth 有り / trade 無し) → DepthUpdate が events() に流れる。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    hub = Stub.instances[0]

    gen = adapter.events()

    async def driver():
        await hub.fire("FD", _fd_fields_first_frame(), recv_ts_ms=1_700_000_000_000)

    drive_task = asyncio.create_task(driver())
    evt = await asyncio.wait_for(gen.__anext__(), timeout=1.0)
    await drive_task

    assert evt.kind == "depth"
    assert evt.instrument_id == "7203.TSE"
    assert evt.ts_ns == 1_700_000_000_000 * 1_000_000
    assert evt.bids[0].price == 2999.0
    assert evt.asks[0].price == 3001.0


async def test_events_yields_trades_update_from_fd_frame(
    monkeypatch, httpx_mock: HTTPXMock,
):
    """FD 2 件目 (DV 進む, buy side) → TradesUpdate + DepthUpdate の 2 件が流れる。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    hub = Stub.instances[0]

    gen = adapter.events()

    async def driver():
        await hub.fire("FD", _fd_fields_first_frame(), recv_ts_ms=1_700_000_000_000)
        await hub.fire("FD", _fd_fields_buy_trade(), recv_ts_ms=1_700_000_001_000)

    drive_task = asyncio.create_task(driver())

    collected = []
    for _ in range(3):  # depth, trade, depth
        evt = await asyncio.wait_for(gen.__anext__(), timeout=1.0)
        collected.append(evt)
    await drive_task

    trades = [e for e in collected if e.kind == "trades"]
    depths = [e for e in collected if e.kind == "depth"]
    assert len(trades) == 1
    assert len(depths) == 2

    t = trades[0]
    assert t.instrument_id == "7203.TSE"
    assert t.price == 3001.0
    assert t.size == 50.0
    assert t.aggressor_side == "buy"
    assert t.ts_ns == 1_700_000_001_000 * 1_000_000


async def test_events_skips_unknown_side_trade(
    monkeypatch, httpx_mock: HTTPXMock,
):
    """ambiguous な trade (side='unknown') は TradesUpdate を出さず depth のみ流す。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    hub = Stub.instances[0]

    gen = adapter.events()

    async def driver():
        await hub.fire("FD", _fd_fields_first_frame(), recv_ts_ms=1_700_000_000_000)
        await hub.fire("FD", _fd_fields_buy_trade(),   recv_ts_ms=1_700_000_001_000)
        await hub.fire("FD", _fd_fields_ambiguous_trade(), recv_ts_ms=1_700_000_002_000)

    drive_task = asyncio.create_task(driver())

    collected = []
    # 期待: depth(1), trade(2), depth(2), depth(3) の 4 件
    for _ in range(4):
        evt = await asyncio.wait_for(gen.__anext__(), timeout=1.0)
        collected.append(evt)
    await drive_task

    trades = [e for e in collected if e.kind == "trades"]
    depths = [e for e in collected if e.kind == "depth"]
    assert len(trades) == 1   # 2 件目の buy のみ
    assert len(depths) == 3   # 1/2/3 件目全部


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A3.3-4: logout closes all hubs + clears registry
# ---------------------------------------------------------------------------


async def test_logout_acloses_all_hubs(monkeypatch, httpx_mock: HTTPXMock):
    """logout は subscribe 中の全 hub に対し aclose() を呼ぶ。"""
    Stub = _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    await adapter.subscribe("9984.TSE", {"trades", "depth"})
    assert len(Stub.instances) == 2

    await adapter.logout()

    for hub in Stub.instances:
        assert hub.aclose_called is True


async def test_logout_clears_hubs_and_processors(monkeypatch, httpx_mock: HTTPXMock):
    """logout 後 _hubs / _processors は空になる。"""
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    await adapter.subscribe("9984.TSE", {"trades", "depth"})
    assert adapter._hubs and adapter._processors

    await adapter.logout()

    assert adapter._hubs == {}
    assert adapter._processors == {}


# ---------------------------------------------------------------------------
# Post-merge review fixes (2026-05-20)
# ---------------------------------------------------------------------------


# HIGH-1: fetch_instruments must surface server-side error envelopes
async def test_fetch_instruments_raises_on_session_expired(
    monkeypatch, httpx_mock: HTTPXMock
):
    """payload with p_errno='2' must raise SessionExpiredError, not return []."""
    from engine.exchanges.tachibana_auth import SessionExpiredError

    adapter = await _login_demo(monkeypatch, httpx_mock)
    err_payload = (
        json.dumps({"p_errno": "2", "sResultCode": "0", "sCLMID": "CLMEventDownload"})
        .encode("shift_jis")
    )
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=err_payload)

    with pytest.raises(SessionExpiredError):
        await adapter.fetch_instruments()


async def test_fetch_instruments_raises_on_api_error(
    monkeypatch, httpx_mock: HTTPXMock
):
    """payload with sResultCode='-1' must raise ApiError."""
    from engine.exchanges.tachibana_auth import ApiError

    adapter = await _login_demo(monkeypatch, httpx_mock)
    err_payload = (
        json.dumps(
            {"p_errno": "0", "sResultCode": "-1", "sResultText": "boom",
             "sCLMID": "CLMEventDownload"}
        ).encode("shift_jis")
    )
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=err_payload)

    with pytest.raises(ApiError):
        await adapter.fetch_instruments()


async def test_fetch_instruments_raises_on_service_out_of_hours(
    monkeypatch, httpx_mock: HTTPXMock
):
    """payload with p_errno='-62' must raise ApiError (service hours)."""
    from engine.exchanges.tachibana_auth import ApiError

    adapter = await _login_demo(monkeypatch, httpx_mock)
    err_payload = (
        json.dumps({"p_errno": "-62", "sResultCode": "0", "sCLMID": "CLMEventDownload"})
        .encode("shift_jis")
    )
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=err_payload)

    with pytest.raises(ApiError):
        await adapter.fetch_instruments()


# MEDIUM-2: events() must terminate after logout()
async def test_events_returns_after_logout(monkeypatch, httpx_mock: HTTPXMock):
    """events() AsyncIterator must terminate (StopAsyncIteration) when logout fires."""
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)

    async def scenario() -> None:
        gen = adapter.events()
        consumer_done = asyncio.Event()

        async def consume() -> None:
            try:
                async for _ in gen:
                    pass
            finally:
                consumer_done.set()

        task = asyncio.create_task(consume())
        # Give the consumer time to park on the queue.
        await asyncio.sleep(0.01)
        await adapter.logout()
        await asyncio.wait_for(consumer_done.wait(), timeout=1.0)
        await task

    await scenario()


# MEDIUM-3: master fetch read-timeout must be generous for multi-MB stream
def test_master_read_timeout_is_at_least_300s():
    from engine.exchanges import tachibana as _tach_mod

    assert getattr(_tach_mod, "_MASTER_READ_TIMEOUT", 0) >= 300


# MEDIUM-4: credentials_source='prompt' wires to run_dialog
async def test_login_prompt_success_loads_session(monkeypatch, tmp_path):
    """run_dialog returns success → adapter loads the saved session and is_logged_in."""
    import json as _json
    from datetime import datetime
    from zoneinfo import ZoneInfo

    today = datetime.now(ZoneInfo("Asia/Tokyo")).date().isoformat()
    session_data = {
        "issued_jst_date": today,
        "url_request": "https://demo/request/",
        "url_master": "https://demo/master/",
        "url_price": "https://demo/price/",
        "url_event": "https://demo/event/",
        "url_event_ws": "wss://demo/event_ws/",
        "zyoutoeki_kazei_c": "1",
        "last_p_no": 1_000_000_000,
    }
    session_file = tmp_path / "session.json"
    session_file.write_text(_json.dumps(session_data), encoding="utf-8")
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(session_file))

    def _fake_run_dialog(env_hint: str) -> dict:
        return {"success": True, "error_code": ""}

    monkeypatch.setattr(
        "engine.exchanges.tachibana_login_flow.run_dialog", _fake_run_dialog
    )

    adapter = TachibanaAdapter(environment="demo")
    await adapter.login(VenueCredentials(credentials_source="prompt"))

    assert adapter.is_logged_in is True
    assert str(adapter._session.url_event_ws) == "wss://demo/event_ws/"


async def test_login_prompt_user_cancel_raises(monkeypatch, tmp_path):
    """run_dialog returns success=False → login raises ValueError (USER_CANCELLED)."""
    monkeypatch.setenv("TACHIBANA_SESSION_PATH", str(tmp_path / "noexist.json"))

    def _fake_run_dialog(env_hint: str) -> dict:
        return {"success": False, "error_code": "USER_CANCELLED"}

    monkeypatch.setattr(
        "engine.exchanges.tachibana_login_flow.run_dialog", _fake_run_dialog
    )

    adapter = TachibanaAdapter(environment="demo")
    with pytest.raises(ValueError, match="USER_CANCELLED"):
        await adapter.login(VenueCredentials(credentials_source="prompt"))


async def test_logout_makes_subscribe_raise(monkeypatch, httpx_mock: HTTPXMock):
    """logout 後 subscribe は session 無しで RuntimeError(login) を投げる。"""
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)
    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    await adapter.logout()

    with pytest.raises(RuntimeError, match="login"):
        await adapter.subscribe("7203.TSE", {"trades", "depth"})


async def test_logout_is_idempotent(monkeypatch, httpx_mock: HTTPXMock):
    """logout を 2 回呼んでも例外を出さない (idempotent)。"""
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)
    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    await adapter.logout()
    await adapter.logout()  # 2 回目: 例外なし
    assert adapter._session is None


# ---------------------------------------------------------------------------
# Phase 8 §3.2 A3.3 fix1 / fix2 (Medium) — EVENT WS URL params & on_connect=reset
# ---------------------------------------------------------------------------


async def test_subscribe_builds_event_ws_url_with_expected_params(
    monkeypatch, httpx_mock: HTTPXMock
):
    """fix1-RED: TickerEventWsHub に渡す ws_url は build_event_url 経由で
    期待 query (p_rid=22, p_board_no=1000, p_gyou_no=1, p_issue_code=<ticker>,
    p_mkt_code=00, p_eno=0, p_evt_cmd=ST,KP,FD) を含む。"""
    adapter = await _login_demo(monkeypatch, httpx_mock)

    captured_urls: list[str] = []
    from engine.exchanges import tachibana as _tach_mod

    class _CaptureHub:
        def __init__(self, ws_url, *, ticker, proxy=None):
            captured_urls.append(ws_url)
            self._subs: dict = {}

        async def subscribe(self, key, callback, *, on_connect=None, on_close=None):
            self._subs[key] = callback

        async def unsubscribe(self, key):
            self._subs.pop(key, None)

        async def aclose(self):
            self._subs.clear()

    monkeypatch.setattr(_tach_mod, "TickerEventWsHub", _CaptureHub)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    assert len(captured_urls) == 1
    url = captured_urls[0]
    assert url.startswith("wss://")
    assert "/event_ws/" in url
    assert "?" in url
    assert "p_rid=22" in url
    assert "p_board_no=1000" in url
    assert "p_gyou_no=1" in url
    assert "p_issue_code=7203" in url
    assert "p_mkt_code=00" in url
    assert "p_eno=0" in url
    # Commas stay RAW: the live server does not percent-decode EVENT params, so
    # %2C silently drops the FD subscription (e-station bug-postmortem 2026-05-01).
    assert "p_evt_cmd=ST,KP,FD" in url
    assert "%2C" not in url


# ---------------------------------------------------------------------------
# Round 2 — Post-merge review fixes (2026-05-20)
# ---------------------------------------------------------------------------


# HIGH-1 (Round 2): logout enqueues a None sentinel; the next login() must
# recreate the queue so the new events() consumer does not pop the stale
# sentinel and terminate immediately.
async def test_events_after_relogin_does_not_see_stale_sentinel(
    monkeypatch, httpx_mock: HTTPXMock,
):
    _install_stub_hub(monkeypatch)
    adapter = await _login_demo(monkeypatch, httpx_mock)
    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    # First session: logout drains a None sentinel into the queue.
    await adapter.logout()

    # Second session: env login again (re-add mock response for the new GET).
    _add_login_response(httpx_mock, _ok_login_payload())
    await adapter.login(VenueCredentials(credentials_source="env"))
    await adapter.subscribe("7203.TSE", {"trades", "depth"})
    hub = _RecordingStubHub.instances[-1]

    gen = adapter.events()

    async def driver():
        await hub.fire("FD", _fd_fields_first_frame(), recv_ts_ms=1_700_000_000_000)

    drive_task = asyncio.create_task(driver())
    # If the stale None leaks the iterator terminates immediately
    # (StopAsyncIteration), failing this wait_for with a different error.
    evt = await asyncio.wait_for(gen.__anext__(), timeout=1.0)
    await drive_task

    assert evt.kind == "depth"


# MEDIUM-1 (Round 2): master fetch must wrap UnicodeDecodeError into ApiError.
async def test_fetch_instruments_wraps_unicode_decode_error_as_api_error(
    monkeypatch, httpx_mock: HTTPXMock,
):
    from engine.exchanges.tachibana_auth import ApiError

    adapter = await _login_demo(monkeypatch, httpx_mock)
    # 0x81 is a Shift-JIS lead byte that requires a valid trail (0x40-0xFC
    # excluding 0x7F). Feeding 0x81 0xFF makes the incremental decoder raise
    # UnicodeDecodeError under errors="strict".
    bad_bytes = b"\x81\xff\x81\xff"
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=bad_bytes)

    with pytest.raises(ApiError) as exc_info:
        await adapter.fetch_instruments()
    assert exc_info.value.code == "MASTER_DECODE_FAILED"
    assert isinstance(exc_info.value.__cause__, UnicodeDecodeError)


# MEDIUM-2 (Round 2): error envelope scan must cover the full records list,
# not just records[0]. Server may interleave a valid record before the
# error envelope.
async def test_fetch_instruments_scans_all_records_for_error_envelope(
    monkeypatch, httpx_mock: HTTPXMock,
):
    from engine.exchanges.tachibana_auth import SessionExpiredError

    adapter = await _login_demo(monkeypatch, httpx_mock)
    # Valid issue record at index 0, error envelope at index 1, no terminator.
    valid = {"sCLMID": "CLMIssueMstKabu", "sIssueCode": "7203",
             "sIssueName": "トヨタ自動車"}
    err = {"p_errno": "2", "sResultCode": "0", "sCLMID": "CLMEventDownload"}
    body = (
        json.dumps(valid, ensure_ascii=False).encode("shift_jis")
        + json.dumps(err, ensure_ascii=False).encode("shift_jis")
    )
    httpx_mock.add_response(url=_MASTER_URL_RE, method="GET", content=body)

    with pytest.raises(SessionExpiredError):
        await adapter.fetch_instruments()


async def test_subscribe_passes_processor_reset_as_on_connect(
    monkeypatch, httpx_mock: HTTPXMock
):
    """fix2-RED (Medium): hub.subscribe(... on_connect=...) には FdFrameProcessor.reset
    と同一の callable が渡されること (WS 再接続時に board snapshot を初期化するため)。"""
    adapter = await _login_demo(monkeypatch, httpx_mock)

    captured_on_connect: list = []
    from engine.exchanges import tachibana as _tach_mod

    class _CaptureOnConnectHub:
        def __init__(self, ws_url, *, ticker, proxy=None):
            self.ticker = ticker
            self._subs: dict = {}

        async def subscribe(self, key, callback, *, on_connect=None, on_close=None):
            captured_on_connect.append(on_connect)
            self._subs[key] = callback

        async def unsubscribe(self, key):
            self._subs.pop(key, None)

        async def aclose(self):
            self._subs.clear()

    monkeypatch.setattr(_tach_mod, "TickerEventWsHub", _CaptureOnConnectHub)

    await adapter.subscribe("7203.TSE", {"trades", "depth"})

    assert len(captured_on_connect) == 1
    processor = adapter._processors["7203"]
    # bound method の `is` 比較は毎回新規 object のため不可。
    # 「processor インスタンスの reset メソッドが渡された」ことを同一性で検証。
    cb = captured_on_connect[0]
    assert cb is not None
    assert cb.__self__ is processor
    assert cb.__func__ is type(processor).reset


# ---------------------------------------------------------------------------
# Phase 9 Step 5: OrderingVenueAdapter — 発注 / 取消 / 訂正 / 口座 / EC
# ---------------------------------------------------------------------------

from engine.live.order_types import AccountSnapshot, OrderResult  # noqa: E402

_REQUEST_URL_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}request/")
_ZANKAI_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}request/.*CLMZanKaiKanougaku")
_GENBUTU_RE = re.compile(rf"^{re.escape(_DEMO_BASE)}request/.*CLMGenbutuKabuList")


class _StubSecretResolver:
    """secret 解決のテストダブル。resolve 呼び出しを (venue, purpose) で記録する。"""

    def __init__(self, secret: str = "pswd") -> None:
        self.calls: list[tuple[str, str]] = []
        self._secret = secret

    async def resolve(self, venue: str, purpose: str) -> str:
        self.calls.append((venue, purpose))
        return self._secret


class _FakeEcWs:
    """口座レベル EC WS のテストダブル。run() は stop までパークする。"""

    last: "_FakeEcWs | None" = None

    def __init__(self, url, stop_event, *, ticker, **kwargs):
        self.url = url
        self._stop = stop_event
        self.ticker = ticker
        self.callback = None
        _FakeEcWs.last = self

    async def run(self, callback, *, on_connect=None):
        self.callback = callback
        await self._stop.wait()


def _order_ok_bytes(*, clmid="CLMKabuNewOrder", order_number="9000015",
                    eigyou_day="20260521") -> bytes:
    return json.dumps({
        "sCLMID": clmid, "p_errno": "0", "sResultCode": "0", "sResultText": "",
        "sOrderNumber": order_number, "sEigyouDay": eigyou_day,
        "sOrderDate": "20260521134803",
    }, ensure_ascii=False).encode("shift_jis")


def _order_rejected_bytes(*, code="21", text="可能額不足") -> bytes:
    return json.dumps(
        {"p_errno": "0", "sResultCode": code, "sResultText": text},
        ensure_ascii=False,
    ).encode("shift_jis")


async def _login_with_hooks(monkeypatch, httpx_mock: HTTPXMock, *, secret="pswd"):
    """env login + 実行 hooks 注入 + EC WS をフェイク化したアダプタを返す。"""
    _FakeEcWs.last = None
    from engine.exchanges import tachibana as _tach_mod
    monkeypatch.setattr(_tach_mod, "TachibanaEventWs", _FakeEcWs)
    monkeypatch.setenv("DEV_TACHIBANA_USER_ID", "uid")
    monkeypatch.setenv("DEV_TACHIBANA_PASSWORD", "pwd")
    _add_login_response(httpx_mock, _ok_login_payload())

    adapter = TachibanaAdapter(environment="demo")
    resolver = _StubSecretResolver(secret)
    events: list = []
    adapter.set_execution_hooks(secret_resolver=resolver, on_order_event=events.append)
    await adapter.login(VenueCredentials(credentials_source="env"))
    return adapter, resolver, events


async def test_submit_order_accepts_and_registers_ref(monkeypatch, httpx_mock: HTTPXMock):
    adapter, resolver, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_ok_bytes())

    res = await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
    )

    assert isinstance(res, OrderResult)
    assert res.status == "ACCEPTED"
    assert res.client_order_id
    assert ("TACHIBANA", "new_order") in resolver.calls
    ref = adapter._orders_ref[res.client_order_id]
    assert ref.order_number == "9000015"
    assert ref.eigyou_day == "20260521"
    # CLMKabuNewOrder が sUrlRequest に送られていること。
    order_reqs = [r for r in httpx_mock.get_requests() if "/request/" in str(r.url)]
    assert len(order_reqs) == 1
    assert "CLMKabuNewOrder" in str(order_reqs[0].url)
    await adapter.logout()


async def test_submit_order_requires_session():
    adapter = TachibanaAdapter(environment="demo")
    with pytest.raises(RuntimeError, match="login"):
        await adapter.submit_order(
            venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
            qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
        )


async def test_submit_order_business_rejection_maps_to_rejected(
    monkeypatch, httpx_mock: HTTPXMock,
):
    adapter, _, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_rejected_bytes())

    res = await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
    )
    assert res.status == "REJECTED"
    assert "21" in (res.reject_reason or "")
    # リジェクトは ref を登録しない。
    assert adapter._orders_ref == {}
    await adapter.logout()


async def test_cancel_order_resolves_two_identifiers_from_registry(
    monkeypatch, httpx_mock: HTTPXMock,
):
    adapter, resolver, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_ok_bytes())
    httpx_mock.add_response(
        url=_REQUEST_URL_RE, method="GET",
        content=_order_ok_bytes(clmid="CLMKabuCancelOrder"),
    )

    placed = await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
    )
    res = await adapter.cancel_order(venue="TACHIBANA", order_id=placed.client_order_id)

    assert res.status == "CANCELED"
    assert ("TACHIBANA", "cancel_order") in resolver.calls
    cancel_req = [r for r in httpx_mock.get_requests() if "CLMKabuCancelOrder" in str(r.url)]
    assert len(cancel_req) == 1
    # 2 識別子が再供給されていること (URL は letters/digits を素通しするため可視)。
    assert "9000015" in str(cancel_req[0].url)  # sOrderNumber
    assert "20260521" in str(cancel_req[0].url)  # sEigyouDay
    await adapter.logout()


async def test_cancel_unknown_order_is_rejected_without_request(
    monkeypatch, httpx_mock: HTTPXMock,
):
    adapter, resolver, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    res = await adapter.cancel_order(venue="TACHIBANA", order_id="does-not-exist")
    assert res.status == "REJECTED"
    assert res.reject_reason == "UNKNOWN_VENUE_ORDER"
    assert resolver.calls == []  # secret も venue 通信も発生しない
    await adapter.logout()


async def test_modify_order_uses_correct_order(monkeypatch, httpx_mock: HTTPXMock):
    adapter, resolver, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_ok_bytes())
    httpx_mock.add_response(
        url=_REQUEST_URL_RE, method="GET",
        content=_order_ok_bytes(clmid="CLMKabuCorrectOrder"),
    )

    placed = await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=2400.0, order_type="LIMIT", time_in_force="DAY",
    )
    res = await adapter.modify_order(
        venue="TACHIBANA", order_id=placed.client_order_id, new_price=2500.0,
    )
    assert res.status == "ACCEPTED"
    assert ("TACHIBANA", "correct_order") in resolver.calls
    correct_req = [r for r in httpx_mock.get_requests() if "CLMKabuCorrectOrder" in str(r.url)]
    assert len(correct_req) == 1
    await adapter.logout()


async def test_fetch_account_parses_buying_power_and_positions(
    monkeypatch, httpx_mock: HTTPXMock,
):
    adapter, _, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(
        url=_ZANKAI_RE, method="GET",
        content=json.dumps(
            {"p_errno": "0", "sResultCode": "0", "sSummaryGenkabuKaituke": "1000000"},
            ensure_ascii=False,
        ).encode("shift_jis"),
    )
    httpx_mock.add_response(
        url=_GENBUTU_RE, method="GET",
        content=json.dumps({
            "p_errno": "0", "sResultCode": "0",
            "aGenbutuKabuList": [{
                "sUriOrderIssueCode": "7203",
                "sUriOrderZanKabuSuryou": "100",
                "sUriOrderGaisanBokaTanka": "2400.0000",
                "sUriOrderGaisanHyoukaSoneki": "3000",
            }],
        }, ensure_ascii=False).encode("shift_jis"),
    )

    snap = await adapter.fetch_account()
    assert isinstance(snap, AccountSnapshot)
    assert snap.buying_power == 1000000.0
    assert len(snap.positions) == 1
    pos = snap.positions[0]
    assert pos.symbol == "7203"
    assert pos.qty == 100
    assert pos.avg_price == 2400.0
    assert pos.unrealized_pnl == 3000.0
    await adapter.logout()


async def test_fetch_account_handles_empty_positions(monkeypatch, httpx_mock: HTTPXMock):
    adapter, _, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(
        url=_ZANKAI_RE, method="GET",
        content=json.dumps(
            {"p_errno": "0", "sResultCode": "0", "sSummaryGenkabuKaituke": "0"},
            ensure_ascii=False,
        ).encode("shift_jis"),
    )
    # R8: 保有ゼロは aGenbutuKabuList が "" で返る。
    httpx_mock.add_response(
        url=_GENBUTU_RE, method="GET",
        content=json.dumps(
            {"p_errno": "0", "sResultCode": "0", "aGenbutuKabuList": ""},
            ensure_ascii=False,
        ).encode("shift_jis"),
    )
    snap = await adapter.fetch_account()
    assert snap.positions == ()
    await adapter.logout()


async def test_login_starts_ec_stream_with_hooks(monkeypatch, httpx_mock: HTTPXMock):
    adapter, _, _ = await _login_with_hooks(monkeypatch, httpx_mock)
    assert _FakeEcWs.last is not None
    # 口座レベル: p_evt_cmd に EC を含む (FD は含まない)。
    assert "EC" in _FakeEcWs.last.url
    assert adapter._ec_task is not None and not adapter._ec_task.done()
    await adapter.logout()
    assert adapter._ec_task is None


def _ec_frame(**overrides):
    from engine.exchanges import tachibana_orders as to
    base = {
        to._EC_ORDER_NUMBER: "9000015", to._EC_TRADE_ID: "1",
        to._EC_NOTIFY_TYPE: "2", to._EC_LAST_PRICE: "2430",
        to._EC_LAST_QTY: "100", to._EC_LEAVES_QTY: "0",
        to._EC_EXEC_DATETIME: "20260521134803",
    }
    base.update(overrides)
    return base


async def test_ec_frame_pushes_order_event_for_known_order(
    monkeypatch, httpx_mock: HTTPXMock,
):
    adapter, _, events = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_ok_bytes())
    placed = await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
    )
    await adapter._dispatch_event_frame("EC", _ec_frame(), 1_700_000_000_000)

    assert len(events) == 1
    ev = events[0]
    assert ev.venue_order_id == "9000015"
    assert ev.client_order_id == placed.client_order_id
    assert ev.status == "FILLED"
    assert ev.filled_qty == 100.0  # 発注 100 - 残 0
    assert ev.avg_price == 2430.0
    # ts は p_OD (約定日時) 由来 (recv_ts ではない)。
    from datetime import datetime, timezone, timedelta
    assert ev.ts_ms == int(datetime(2026, 5, 21, 13, 48, 3,
                           tzinfo=timezone(timedelta(hours=9))).timestamp() * 1000)
    await adapter.logout()


async def test_ec_partial_fill_uses_leaves_qty(monkeypatch, httpx_mock: HTTPXMock):
    """部分約定: 累計約定数量 = 発注数量 - 残数量。status は PARTIALLY_FILLED。"""
    adapter, _, events = await _login_with_hooks(monkeypatch, httpx_mock)
    httpx_mock.add_response(url=_REQUEST_URL_RE, method="GET", content=_order_ok_bytes())
    await adapter.submit_order(
        venue="TACHIBANA", instrument_id="7203.TSE", side="BUY",
        qty=100.0, price=None, order_type="MARKET", time_in_force="DAY",
    )
    # 30 株約定・残 70。
    await adapter._dispatch_event_frame(
        "EC", _ec_frame(**{"p_DSU": "30", "p_ZSU": "70"}), 1_700_000_000_000,
    )
    assert len(events) == 1
    assert events[0].status == "PARTIALLY_FILLED"
    assert events[0].filled_qty == 30.0  # 100 - 70
    await adapter.logout()


async def test_non_ec_frame_does_not_push(monkeypatch, httpx_mock: HTTPXMock):
    adapter, _, events = await _login_with_hooks(monkeypatch, httpx_mock)
    await adapter._dispatch_event_frame("KP", {}, 1_700_000_000_000)
    await adapter._dispatch_event_frame("FD", {"p_1_DPP": "3000"}, 1_700_000_000_000)
    assert events == []
    await adapter.logout()


async def test_duplicate_ec_frame_is_pushed_once(monkeypatch, httpx_mock: HTTPXMock):
    """EC は接続毎に全件再送される。同一 (vid,trade_id,nt) の再送は push しない。"""
    adapter, _, events = await _login_with_hooks(monkeypatch, httpx_mock)
    ec = _ec_frame()
    await adapter._dispatch_event_frame("EC", ec, 1_700_000_000_000)
    await adapter._dispatch_event_frame("EC", dict(ec), 1_700_000_001_000)  # 再送
    assert len(events) == 1
    # 別の約定枝番 (trade_id) は新規イベント → push される。
    await adapter._dispatch_event_frame(
        "EC", _ec_frame(**{"p_EDA": "2", "p_DSU": "50", "p_ZSU": "50"}),
        1_700_000_002_000,
    )
    assert len(events) == 2
    await adapter.logout()


# ---------------------------------------------------------------------------
# SS=システムステータス 閉局検知 (Phase 9 Step 7, §3.5)
# ⚠️ TENTATIVE: EVENT フレームでの sSystemStatus/sLoginKyokaKubun の prefix は Demo 未検証。
# ---------------------------------------------------------------------------


def _ss_adapter():
    """on_venue_logout だけ wire した軽量 adapter (login 不要・SS ハンドラ単体検証)。"""
    from engine.exchanges.tachibana import TachibanaAdapter
    adapter = TachibanaAdapter(environment="demo")
    fired: list[str] = []
    adapter._on_venue_logout = fired.append
    return adapter, fired


async def test_ss_closed_fires_venue_logout_once():
    """SS 閉局 (sSystemStatus=0) → on_venue_logout("TACHIBANA") を 1 回通知。"""
    adapter, fired = _ss_adapter()
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 1_700_000_000_000)
    assert fired == ["TACHIBANA"]


async def test_ss_open_does_not_fire():
    """SS 開局 (sSystemStatus=1, ログイン許可=1) → 通知しない。"""
    adapter, fired = _ss_adapter()
    await adapter._dispatch_event_frame(
        "SS", {"sSystemStatus": "1", "sLoginKyokaKubun": "1"}, 1_700_000_000_000,
    )
    assert fired == []


async def test_ss_closed_resend_is_debounced():
    """SS は接続毎に初回再送される。閉局の再送では連打しない (open→closed 遷移のみ)。"""
    adapter, fired = _ss_adapter()
    # まず開局を観測 (login 直後の初回再送)。
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "1"}, 1)
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 2)  # 閉局遷移
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 3)  # 再送
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 4)  # 再送
    assert fired == ["TACHIBANA"]


async def test_ss_recovery_rearms():
    """閉局→開局→閉局 で再通知される (debounce 解除)。"""
    adapter, fired = _ss_adapter()
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 1)
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "1"}, 2)  # 復旧
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 3)  # 再閉局
    assert fired == ["TACHIBANA", "TACHIBANA"]


async def test_ss_login_not_permitted_is_logout():
    """開局でもログイン不許可 (sLoginKyokaKubun=0) は要再ログインとみなす。"""
    adapter, fired = _ss_adapter()
    await adapter._dispatch_event_frame(
        "SS", {"sSystemStatus": "1", "sLoginKyokaKubun": "0"}, 1,
    )
    assert fired == ["TACHIBANA"]


async def test_ss_unrecognized_fields_are_ignored():
    """判別フィールドが無い SS フレームは安全側で無視する (prefix 不一致など)。"""
    adapter, fired = _ss_adapter()
    await adapter._dispatch_event_frame("SS", {"p_unknown": "x"}, 1)
    assert fired == []


async def test_ss_unrecognized_fields_log_actual_keys_once(caplog):
    """既知フィールド欠落時、実フィールド名を 1 度だけ warning (Demo で prefix 確定用)。
    再送でスパムしない (本番で p_* 変種だった場合に恒常スパムさせないため)。"""
    import logging
    adapter, fired = _ss_adapter()
    with caplog.at_level(logging.WARNING, logger="engine.exchanges.tachibana"):
        await adapter._dispatch_event_frame("SS", {"p_GSCD": "0", "p_foo": "1"}, 1)
        await adapter._dispatch_event_frame("SS", {"p_GSCD": "0", "p_foo": "1"}, 2)
    diag = [r for r in caplog.records if "actual field keys" in r.getMessage()]
    assert len(diag) == 1, "診断 warning はセッション 1 回だけ"
    assert "p_GSCD" in diag[0].getMessage() and "p_foo" in diag[0].getMessage()
    assert fired == []  # 通知はしない (安全側)


async def test_ss_does_not_push_order_event():
    """SS フレームは OrderEvent を push しない。"""
    adapter, fired = _ss_adapter()
    events: list = []
    adapter._on_order_event = events.append
    await adapter._dispatch_event_frame("SS", {"sSystemStatus": "0"}, 1)
    assert events == []
    assert fired == ["TACHIBANA"]
