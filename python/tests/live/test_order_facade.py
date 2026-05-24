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


def test_place_event_carries_static_order_attributes() -> None:
    """issue #29 Slice3a: place の event は symbol/side/qty/price を保持する。

    `OrderEvent` (proto) は元来 ids+status+fill のみで symbol/side/qty/price を
    持たなかった。GetOrders による接続/再起動後の seed で完全な注文行を復元するには
    facade が発注時の静的属性を OrderEventData に載せる必要がある（symbol=instrument_id）。
    """

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        return await facade.place(
            venue="MOCK",
            instrument_id="7203.TSE",
            side="BUY",
            qty=100.0,
            order_type="LIMIT",
            time_in_force="DAY",
            price=2500.0,
        )

    ev = asyncio.run(scenario())
    assert ev.symbol == "7203.TSE"
    assert ev.side == "BUY"
    assert ev.qty == 100.0
    assert ev.price == 2500.0


def test_place_market_event_has_no_price() -> None:
    """MARKET 発注の event は price=None（指値なし）。symbol/side/qty は保持。"""

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        return await facade.place(
            venue="MOCK",
            instrument_id="6758.TSE",
            side="SELL",
            qty=300.0,
            order_type="MARKET",
            time_in_force="DAY",
            price=2500.0,  # MARKET なので注文行の price には載らない
        )

    ev = asyncio.run(scenario())
    assert ev.symbol == "6758.TSE"
    assert ev.side == "SELL"
    assert ev.qty == 300.0
    assert ev.price is None


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


def test_cancel_event_carries_static_attributes() -> None:
    """issue #29 Slice3a: cancel の CANCELED event も元注文の symbol/side/qty/price を保持。"""

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
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
        return await facade.cancel(venue="MOCK", order_id=placed.order_id)

    canceled = asyncio.run(scenario())
    assert canceled.status == "CANCELED"
    assert canceled.symbol == "7203.TSE"
    assert canceled.side == "BUY"
    assert canceled.qty == 100.0
    assert canceled.price == 2500.0


def test_modify_event_preserves_symbol_side_and_applies_new_price() -> None:
    """issue #29 Slice3a: modify の event は symbol/side/qty を保持し新 price を反映。"""

    async def scenario() -> tuple[OrderEventData, OrderEventData]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        modified = await facade.modify(
            venue="MOCK", order_id=placed.order_id, new_price=2600.0
        )
        return placed, modified

    placed, modified = asyncio.run(scenario())
    assert modified.symbol == placed.symbol == "7203.TSE"
    assert modified.side == placed.side == "BUY"
    assert modified.qty == placed.qty == 100.0  # 未指定なので据え置き
    assert modified.price == 2600.0  # 新指値を反映


def test_modify_new_qty_updates_qty_keeps_price() -> None:
    """issue #29 Slice3a: new_qty のみの modify は qty を更新し price は据え置く。"""

    async def scenario() -> OrderEventData:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)  # LIMIT 2500, qty 100
        return await facade.modify(venue="MOCK", order_id=placed.order_id, new_qty=50.0)

    modified = asyncio.run(scenario())
    assert modified.qty == 50.0
    assert modified.price == 2500.0  # 未指定なので据え置き


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


# --- modify ----------------------------------------------------------------


async def _placed_working_order(facade: ManualOrderFacade, adapter: MockVenueAdapter) -> OrderEventData:
    """訂正可能（非終端）な working order を 1 件発注して返す。"""
    adapter.set_next_order_outcome(status="ACCEPTED", filled_qty=0.0)
    return await facade.place(
        venue="MOCK",
        instrument_id="7203.TSE",
        side="BUY",
        qty=100.0,
        order_type="LIMIT",
        time_in_force="DAY",
        price=2500.0,
    )


def test_modify_known_working_order_updates_store() -> None:
    """track 済み working order の modify は ACCEPTED event を返し store を更新する。"""

    async def scenario() -> tuple[OrderEventData, OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        modified = await facade.modify(
            venue="MOCK", order_id=placed.order_id, new_price=2600.0
        )
        return placed, modified, facade

    placed, modified, facade = asyncio.run(scenario())
    assert modified.status == "ACCEPTED"
    assert modified.order_id == placed.order_id
    assert modified.client_order_id == placed.order_id
    # 約定量は維持（mock modify は filled を巻き戻さない）
    assert modified.filled_qty == placed.filled_qty
    assert facade.get_status(placed.order_id).status == "ACCEPTED"


def test_modify_unknown_order_raises() -> None:
    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        await facade.modify(venue="MOCK", order_id="nope", new_price=10.0)

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "UNKNOWN_ORDER_ID"


def test_modify_terminal_order_raises_not_modifiable() -> None:
    """終端状態（FILLED 等）の注文は venue に送らず ORDER_NOT_MODIFIABLE。"""

    async def scenario() -> None:
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
        await facade.modify(venue="MOCK", order_id=placed.order_id, new_qty=50.0)

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "ORDER_NOT_MODIFIABLE"


def test_modify_nothing_to_modify_raises() -> None:
    """new_price も new_qty も None なら NOTHING_TO_MODIFY。"""

    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        await facade.modify(venue="MOCK", order_id=placed.order_id)

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "NOTHING_TO_MODIFY"


@pytest.mark.parametrize("bad_price", [0.0, -1.0, float("nan"), float("inf")])
def test_modify_invalid_price_raises(bad_price) -> None:
    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        await facade.modify(venue="MOCK", order_id=placed.order_id, new_price=bad_price)

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "INVALID_PRICE"


@pytest.mark.parametrize("bad_qty", [0.0, -5.0, float("nan"), float("inf")])
def test_modify_invalid_qty_raises(bad_qty) -> None:
    async def scenario() -> None:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        await facade.modify(venue="MOCK", order_id=placed.order_id, new_qty=bad_qty)

    with pytest.raises(OrderFacadeError) as exc:
        asyncio.run(scenario())
    assert exc.value.error_code == "INVALID_QTY"


def test_modify_rejected_raises_and_leaves_order_intact() -> None:
    """venue が訂正拒否したら MODIFY_REJECTED を raise し、元注文 state は不変。"""

    async def scenario() -> tuple[OrderEventData, ManualOrderFacade]:
        adapter = await _logged_in_adapter()
        facade = ManualOrderFacade(adapter)
        placed = await _placed_working_order(facade, adapter)
        adapter.set_next_modify_outcome(status="REJECTED", reject_reason="too late")
        with pytest.raises(OrderFacadeError) as exc:
            await facade.modify(venue="MOCK", order_id=placed.order_id, new_price=2600.0)
        assert exc.value.error_code == "MODIFY_REJECTED"
        return placed, facade

    placed, facade = asyncio.run(scenario())
    # 訂正拒否後も store 上の状態は元のまま（ACCEPTED を維持）
    assert facade.get_status(placed.order_id).status == placed.status == "ACCEPTED"
