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


async def test_login_session_cache_raises_not_implemented():
    creds = VenueCredentials(credentials_source="session_cache")
    with pytest.raises(NotImplementedError):
        await TachibanaAdapter().login(creds)


async def test_login_prompt_raises_not_implemented():
    creds = VenueCredentials(credentials_source="prompt")
    with pytest.raises(NotImplementedError):
        await TachibanaAdapter().login(creds)


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
    assert ws_url == adapter._session.url_event_ws


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
