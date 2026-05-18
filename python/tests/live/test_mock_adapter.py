"""MockVenueAdapter spec (Phase 8 Step C — deterministic mock for live_runner tests).

Protocol 適合の最小確認のみ。login/state, fetch, subscribe/inject は
後続の C-2 以降で追加する。
"""
from __future__ import annotations

import asyncio

from engine.live.adapter import InstrumentRaw, LiveVenueAdapter, VenueCredentials
from engine.live.mock_adapter import MockVenueAdapter


def test_mock_adapter_satisfies_protocol() -> None:
    """runtime_checkable LiveVenueAdapter Protocol を満たし、venue_id は "MOCK"。"""
    adapter = MockVenueAdapter()
    assert isinstance(adapter, LiveVenueAdapter)
    assert adapter.venue_id == "MOCK"


def test_mock_adapter_login_logout_toggles_is_logged_in() -> None:
    """login(creds) 後に is_logged_in=True、logout() 後に False になる。

    C-2 RED: MockVenueAdapter に is_logged_in: bool 状態を持たせ、
    login/logout で遷移することを保証する。credentials_source は
    "env" 固定（mock は実 credential を見ない）。
    """
    adapter = MockVenueAdapter()
    creds = VenueCredentials(credentials_source="env", environment_hint="demo")

    # 初期状態は未ログイン
    assert adapter.is_logged_in is False

    # login 後は True
    asyncio.run(adapter.login(creds))
    assert adapter.is_logged_in is True

    # logout 後は False に戻る
    asyncio.run(adapter.logout())
    assert adapter.is_logged_in is False


def test_mock_adapter_fetch_instruments_is_deterministic() -> None:
    """fetch_instruments() は決定的に最低 2 件返し、連続呼び出しで同一。

    C-3 RED: live_runner テストで再現可能な instrument 集合が要るため、
    MockVenueAdapter.fetch_instruments() に固定リストを返す実装を入れる。
    要件:
      - 件数 >= 2
      - 全要素が InstrumentRaw インスタンス
      - market は全件 "TSE"
      - code に重複が無い
      - 2 回呼び出して内容が完全一致（決定的）
    """
    adapter = MockVenueAdapter()

    first = asyncio.run(adapter.fetch_instruments())
    second = asyncio.run(adapter.fetch_instruments())

    assert len(first) >= 2
    assert all(isinstance(x, InstrumentRaw) for x in first)
    assert all(x.market == "TSE" for x in first)
    codes = [x.code for x in first]
    assert len(set(codes)) == len(codes), f"code 重複: {codes}"
    assert first == second  # InstrumentRaw は BaseModel なので == で field 比較される


def test_mock_adapter_subscribe_then_inject_tick_flows_via_events() -> None:
    """C-4 RED: subscribe 後に inject_tick した TradesUpdate が events() から流れる。

    要件:
      - subscribe(instrument_id, {"trades"}) 済みの instrument に inject_tick すると、
        events() の async iterator から同じ TradesUpdate インスタンス（field 一致）が
        1 件取得できる。
      - inject_tick は MockVenueAdapter 固有の同期メソッド（Protocol 外、追加 OK）で、
        引数は LiveEvent（ここでは TradesUpdate）1 件。
      - events() は無限待ちにならないよう asyncio.wait_for(timeout=1.0) で受ける。
    """
    from engine.live.adapter import TradesUpdate

    adapter = MockVenueAdapter()
    instrument_id = "7203.TSE"
    tick = TradesUpdate(
        kind="trades",
        instrument_id=instrument_id,
        ts_ns=1_700_000_000_000_000_000,
        price=2500.0,
        size=100.0,
        aggressor_side="buy",
    )

    async def scenario() -> TradesUpdate:
        await adapter.subscribe(instrument_id, {"trades"})
        adapter.inject_tick(tick)  # 同期メソッド想定（subscribe 後に内部 queue へ push）
        it = adapter.events().__aiter__()
        evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        return evt  # type: ignore[return-value]

    received = asyncio.run(scenario())
    assert received == tick


def test_mock_adapter_unsubscribe_stops_flow_to_events() -> None:
    """C-4b RED: unsubscribe 後に inject_tick した event は events() に流れない。

    要件:
      - subscribe(instrument_id, {"trades"}) → unsubscribe(instrument_id)
        の後に inject_tick しても、events() の async iterator は何も
        受け取らない（= asyncio.wait_for(timeout=0.2) で TimeoutError）。
      - これにより MockVenueAdapter.inject_tick の
        `if event.instrument_id in self._subscribed:` ガードが
        unsubscribe 後に False になっていることを保証する。
    """
    from engine.live.adapter import TradesUpdate

    adapter = MockVenueAdapter()
    instrument_id = "7203.TSE"
    tick = TradesUpdate(
        kind="trades",
        instrument_id=instrument_id,
        ts_ns=1_700_000_000_000_000_000,
        price=2500.0,
        size=100.0,
        aggressor_side="buy",
    )

    async def scenario() -> None:
        await adapter.subscribe(instrument_id, {"trades"})
        await adapter.unsubscribe(instrument_id)
        adapter.inject_tick(tick)  # unsubscribe 済みなので queue には積まれない想定
        it = adapter.events().__aiter__()
        await asyncio.wait_for(it.__anext__(), timeout=0.2)

    import pytest
    with pytest.raises(asyncio.TimeoutError):
        asyncio.run(scenario())


def test_mock_adapter_emit_depth_snapshot_flows_via_events() -> None:
    """C-5 RED: emit_depth_snapshot after subscribe({"depth"}) flows DepthUpdate via events().

    Requirements:
      - For an instrument with subscribe(instrument_id, {"depth"}),
        calling emit_depth_snapshot(instrument_id, ts_ns, bids, asks)
        must yield one DepthUpdate (kind="depth") from events().
      - bids/asks are passed as list[DepthLevel] directly (thin API, no conversion).
      - The received event matches input on kind, instrument_id, ts_ns, bids, asks.
      - subscribe gating applies the same as inject_tick; unsubscribed case is
        covered by C-4/C-4b and is out of scope here.
    """
    from engine.live.adapter import DepthLevel, DepthUpdate

    adapter = MockVenueAdapter()
    instrument_id = "7203.TSE"
    ts_ns = 1_700_000_000_000_000_000
    bids = [DepthLevel(price=2499.5, size=300.0), DepthLevel(price=2499.0, size=500.0)]
    asks = [DepthLevel(price=2500.0, size=200.0), DepthLevel(price=2500.5, size=400.0)]

    async def scenario() -> DepthUpdate:
        await adapter.subscribe(instrument_id, {"depth"})
        adapter.emit_depth_snapshot(instrument_id, ts_ns, bids, asks)
        it = adapter.events().__aiter__()
        evt = await asyncio.wait_for(it.__anext__(), timeout=1.0)
        return evt  # type: ignore[return-value]

    received = asyncio.run(scenario())
    assert isinstance(received, DepthUpdate)
    assert received.kind == "depth"
    assert received.instrument_id == instrument_id
    assert received.ts_ns == ts_ns
    assert list(received.bids) == bids
    assert list(received.asks) == asks
