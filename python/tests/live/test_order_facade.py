"""ManualOrderFacade spec (Phase 9 Step 2 — 軽量手動発注 facade)。

facade は transport 非依存（proto を import しない）。adapter.submit_order /
cancel_order への委譲、OrderResult → OrderEventData の正規化、in-memory order
store（GetOrderStatus 用）、検証エラー / 取消拒否のセマンティクスを固定する。
"""
from __future__ import annotations

import asyncio

import pytest

from engine.live.adapter import VenueCredentials
from engine.live.mock_adapter import MockVenueAdapter
from engine.live.order_facade import ManualOrderFacade, OrderFacadeError
from engine.live.order_types import OrderEventData, OrderResult


async def _logged_in_adapter() -> MockVenueAdapter:
    adapter = MockVenueAdapter()
    await adapter.login(VenueCredentials(credentials_source="env", environment_hint="demo"))
    return adapter


def test_place_default_filled_returns_event_and_tracks() -> None:
    """仕込み無しの place は FILLED event を返し、store に track される。"""

    async def scenario() -> tuple[OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        ev = await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="LIMIT",
            time_in_force="DAY",
            price=2500.0,
        )
        return ev, facade

    ev, facade = asyncio.run(scenario())
    assert isinstance(ev, OrderEventData)
    assert ev.status == "FILLED"
    assert ev.filled_qty == 100.0
    assert ev.avg_price == 2500.0
    assert ev.client_order_id
    assert ev.order_id == ev.client_order_id
    assert ev.venue_order_id == ""
    assert ev.ts_ms > 0
    # tracked for GetOrderStatus
    assert facade.get_status(ev.order_id) == ev


def test_place_market_does_not_forward_price() -> None:
    """MARKET 発注は price を venue に渡さない（avg_price は約定価格 None→0.0）。"""

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        return await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="SELL",
            qty=100.0,
            order_type="MARKET",
            time_in_force="DAY",
            price=2500.0,  # MARKET なので無視されるべき
        )

    ev = asyncio.run(scenario())
    assert ev.status == "FILLED"
    # mock は price=None のとき avg_price=None → facade が 0.0 に正規化
    assert ev.avg_price == 0.0


def test_place_rejected_outcome_is_event_not_exception() -> None:
    """venue REJECTED は例外ではなく status=REJECTED の event として返る。"""

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        adapter.set_next_order_outcome(status="REJECTED", reject_reason="margin")
        facade = ManualOrderFacade(adapter)
        return await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="MARKET",
            time_in_force="DAY",
        )

    ev = asyncio.run(scenario())
    assert ev.status == "REJECTED"
    assert ev.filled_qty == 0.0


@pytest.mark.parametrize(
    "kwargs,code",
    [
        ({"side": "HOLD", "order_type": "MARKET", "qty": 100.0}, "INVALID_SIDE"),
        ({"side": "BUY", "order_type": "STOP", "qty": 100.0}, "INVALID_ORDER_TYPE"),
        ({"side": "BUY", "order_type": "MARKET", "qty": 0.0}, "INVALID_QTY"),
        ({"side": "BUY", "order_type": "LIMIT", "qty": 100.0}, "INVALID_PRICE"),
    ],
)
def test_place_validation_raises_order_facade_error(kwargs, code) -> None:
    """不正パラメータは OrderFacadeError(error_code) を raise（adapter 未到達）。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            time_in_force="DAY",
            **kwargs,  # price は LIMIT ケースでも未指定 → INVALID_PRICE
        )

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == code


def test_cancel_known_order_returns_canceled_preserving_fills() -> None:
    """track 済み order の cancel は CANCELED + 既存約定量を維持。"""

    async def scenario() -> tuple[OrderEventData, OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        adapter.set_next_order_outcome(status="PARTIALLY_FILLED", filled_qty=40.0)
        facade = ManualOrderFacade(adapter)
        placed = await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="LIMIT",
            time_in_force="DAY",
            price=2500.0,
        )
        canceled = await facade.cancel(venue="MOCK", order_id=placed.order_id)
        return placed, canceled, facade

    placed, canceled, facade = asyncio.run(scenario())
    assert canceled.status == "CANCELED"
    assert canceled.filled_qty == placed.filled_qty == 40.0
    # store 上も終端状態へ更新
    assert facade.get_status(placed.order_id).status == "CANCELED"


def test_cancel_unknown_order_raises() -> None:
    """track されていない order_id の cancel は UNKNOWN_ORDER_ID。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        await facade.cancel(venue="MOCK", order_id="nope")

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "UNKNOWN_ORDER_ID"


def test_cancel_rejected_raises_and_leaves_order_intact() -> None:
    """venue が取消拒否したら CANCEL_REJECTED を raise し、元注文 state は不変。"""

    async def scenario() -> tuple[OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        # working (ACCEPTED) order — cancelable, so the cancel reaches the venue
        # which then rejects it (a FILLED order would short-circuit earlier).
        adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
        placed = await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="MARKET",
            time_in_force="DAY",
        )
        adapter.set_next_cancel_outcome(status="REJECTED", reject_reason="already filled")
        with pytest.raises(OrderFacadeError) as exc:
            await facade.cancel(venue="MOCK", order_id=placed.order_id)
        assert exc.value.error_code == "CANCEL_REJECTED"
        return placed, facade

    placed, facade = asyncio.run(scenario())
    # 取消拒否後も store 上の状態は元のまま（CANCELED に遷移していない）
    assert facade.get_status(placed.order_id).status == placed.status == "ACCEPTED"


def test_cancel_terminal_order_raises_not_cancelable() -> None:
    """終端状態（FILLED 等）の注文は venue に送らず ORDER_NOT_CANCELABLE。"""

    async def scenario() -> tuple[OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await facade.place(  # default outcome = FILLED (terminal)
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="MARKET",
            time_in_force="DAY",
        )
        with pytest.raises(OrderFacadeError) as exc:
            await facade.cancel(venue="MOCK", order_id=placed.order_id)
        assert exc.value.error_code == "ORDER_NOT_CANCELABLE"
        return placed, facade

    placed, facade = asyncio.run(scenario())
    # 元の終端状態は不変（CANCELED へ書き換わっていない）
    assert facade.get_status(placed.order_id).status == "FILLED"


@pytest.mark.parametrize("bad_qty", [float("nan"), float("inf"), float("-inf")])
def test_place_rejects_non_finite_qty(bad_qty) -> None:
    """NaN/Inf qty は `<= 0` を素通りするので isfinite で弾く（INVALID_QTY）。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=bad_qty,
            order_type="MARKET",
            time_in_force="DAY",
        )

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "INVALID_QTY"


def test_place_rejects_empty_instrument() -> None:
    """空の instrument_id は adapter 未到達で INVALID_INSTRUMENT。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        await facade.place(
            venue="MOCK",
            instrument_id="",
            side="BUY",
            qty=100.0,
            order_type="MARKET",
            time_in_force="DAY",
        )

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "INVALID_INSTRUMENT"


def test_order_result_rejects_non_nautilus_status() -> None:
    """OrderResult.status は Nautilus OrderStatus 名に限定（"CANCELLED" 等の typo を弾く）。"""
    import pydantic

    OrderResult(status="FILLED", filled_qty=1.0, avg_price=1.0, client_order_id="x")
    with pytest.raises(pydantic.ValidationError):
        OrderResult(
            status="CANCELLED",  # two L's — common Nautilus typo
            filled_qty=0.0,
            avg_price=None,
            client_order_id="x",
        )


def test_get_status_unknown_returns_none() -> None:
    adapter = MockVenueAdapter()
    facade = ManualOrderFacade(adapter)
    assert facade.get_status("missing") is None
