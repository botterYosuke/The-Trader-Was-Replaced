"""Phase 9 Step 6: KabuStationAdapter 発注経路 (OrderingVenueAdapter) のテスト。

httpx_mock で /sendorder /cancelorder /orders /wallet/cash /positions をモックし、
- 新規発注 (ACCEPTED / REJECTED / Result=-1 → KabuApiError)
- 取消 (CANCELED / 未知 / 拒否)
- 訂正 = 取消→新規変換 (全成功 / 取消失敗 / 新規失敗の補償)
- 口座同期 (買付余力 + 現物保有)
- GET /orders polling → OrderEvent push + dedup
- set_execution_hooks の server_grpc 互換 (secret_resolver を受理して無視)
を検証する。Password を一切送らないこと (R3) も assert する。
"""

import asyncio
import json

import pytest
from pytest_httpx import HTTPXMock

from engine.exchanges.kabusapi import KabuStationAdapter, _KabuOrderRef
from engine.exchanges.kabusapi_auth import KabuApiError
from engine.exchanges.kabusapi_url import endpoint
from engine.live.adapter import OrderingVenueAdapter

_SEND = endpoint("sendorder", env="verify")
_CANCEL = endpoint("cancelorder", env="verify")
_WALLET = endpoint("wallet/cash", env="verify")


async def _noop_sleep(_seconds: float) -> None:
    return None


def _logged_in_adapter() -> KabuStationAdapter:
    a = KabuStationAdapter(environment="verify")
    a._token = "tkn"
    a._rate_limit_sleep = _noop_sleep  # 流量待ちで実時間を消費しない
    return a


def _ref(client_order_id="C1", order_id="O1", qty=100.0, price=2500.0) -> _KabuOrderRef:
    return _KabuOrderRef(
        client_order_id=client_order_id, order_id=order_id, symbol="7203",
        exchange=1, side="BUY", qty=qty, price=price, order_type="LIMIT",
        time_in_force="DAY", account_type=4,
    )


# ---------------------------------------------------------------------------
# Protocol + hooks
# ---------------------------------------------------------------------------


def test_is_ordering_venue_adapter():
    assert isinstance(KabuStationAdapter(), OrderingVenueAdapter)


def test_set_execution_hooks_accepts_secret_resolver_kwarg():
    """server_grpc は Tachibana と同じ呼び出し口 (secret_resolver=...) を使う。
    kabu は Password 不要なので resolver を受理して無視する。"""
    a = KabuStationAdapter()
    a.set_execution_hooks(secret_resolver=object(), on_order_event=lambda e: None)
    assert a._on_order_event is not None


# ---------------------------------------------------------------------------
# submit_order
# ---------------------------------------------------------------------------


async def test_submit_order_success_registers_and_accepts(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O1"})
    a = _logged_in_adapter()
    res = await a.submit_order(
        venue="KABU", instrument_id="7203.TSE", side="BUY", qty=100,
        price=2500.0, order_type="LIMIT", time_in_force="DAY",
    )
    assert res.status == "ACCEPTED"
    assert res.client_order_id in a._orders_ref
    assert a._orders_ref[res.client_order_id].order_id == "O1"
    assert a._order_id_to_cid["O1"] == res.client_order_id


async def test_submit_order_sends_no_password_and_correct_header(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O1"})
    a = _logged_in_adapter()
    await a.submit_order(
        venue="KABU", instrument_id="7203.TSE", side="BUY", qty=100,
        price=2500.0, order_type="LIMIT", time_in_force="DAY",
    )
    req = next(r for r in httpx_mock.get_requests() if str(r.url).endswith("/sendorder"))
    body = json.loads(req.content)
    assert "Password" not in body and "sSecondPassword" not in body
    assert body["Side"] == "2" and body["CashMargin"] == 1
    assert req.headers["X-API-KEY"] == "tkn"


async def test_submit_order_business_reject_returns_rejected(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 21, "Message": "no bp"})
    a = _logged_in_adapter()
    res = await a.submit_order(
        venue="KABU", instrument_id="7203.TSE", side="BUY", qty=100,
        price=2500.0, order_type="LIMIT", time_in_force="DAY",
    )
    assert res.status == "REJECTED"
    assert "21" in (res.reject_reason or "")
    assert a._orders_ref == {}  # リジェクトは登録しない


async def test_submit_order_system_error_raises(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": -1})
    a = _logged_in_adapter()
    with pytest.raises(KabuApiError):
        await a.submit_order(
            venue="KABU", instrument_id="7203.TSE", side="BUY", qty=100,
            price=2500.0, order_type="LIMIT", time_in_force="DAY",
        )


async def test_submit_order_requires_login():
    a = KabuStationAdapter()
    with pytest.raises(RuntimeError):
        await a.submit_order(
            venue="KABU", instrument_id="7203.TSE", side="BUY", qty=100,
            price=2500.0, order_type="LIMIT", time_in_force="DAY",
        )


# ---------------------------------------------------------------------------
# cancel_order
# ---------------------------------------------------------------------------


async def test_cancel_order_success(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    a = _logged_in_adapter()
    a._register_order(_ref())
    res = await a.cancel_order(venue="KABU", order_id="C1")
    assert res.status == "CANCELED"
    req = next(r for r in httpx_mock.get_requests() if str(r.url).endswith("/cancelorder"))
    body = json.loads(req.content)
    assert body == {"OrderID": "O1"}  # Password 不要


async def test_cancel_order_unknown_returns_rejected():
    a = _logged_in_adapter()
    res = await a.cancel_order(venue="KABU", order_id="ZZZ")
    assert res.status == "REJECTED"
    assert res.reject_reason == "UNKNOWN_VENUE_ORDER"


async def test_cancel_order_venue_reject(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 4, "Message": "too late"})
    a = _logged_in_adapter()
    a._register_order(_ref())
    res = await a.cancel_order(venue="KABU", order_id="C1")
    assert res.status == "REJECTED"


# ---------------------------------------------------------------------------
# modify_order (取消 → 新規変換)
# ---------------------------------------------------------------------------


async def test_modify_order_full_success_remaps_order_id(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 0,
               "Details": [{"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O2"})
    a = _logged_in_adapter()
    a._register_order(_ref())
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "ACCEPTED"
    assert a._orders_ref["C1"].order_id == "O2"
    assert a._orders_ref["C1"].price == 2600.0
    assert a._order_id_to_cid == {"O2": "C1"}


async def test_modify_order_cancel_failed_keeps_original(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 4, "Message": "too late"})
    a = _logged_in_adapter()
    a._register_order(_ref())
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "REJECTED"
    assert "原注文は残っています" in (res.reject_reason or "")
    assert a._orders_ref["C1"].order_id == "O1"  # 元注文は不変


async def test_modify_order_new_failed_returns_canceled(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 0,
               "Details": [{"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 21, "Message": "no bp"})
    a = _logged_in_adapter()
    a._register_order(_ref())
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "CANCELED"
    assert "原注文は取消済み" in (res.reject_reason or "")
    assert "C1" not in a._orders_ref  # 元注文は終端化されレジストリから除去


async def test_modify_order_unknown_returns_rejected():
    a = _logged_in_adapter()
    res = await a.modify_order(venue="KABU", order_id="ZZZ", new_price=1.0)
    assert res.status == "REJECTED"
    assert res.reject_reason == "UNKNOWN_VENUE_ORDER"


async def test_modify_after_partial_fill_resubmits_only_remainder(httpx_mock: HTTPXMock):
    """訂正前に 40/100 約定していたら、再発注は残数量 60 のみ (full qty 再発注は
    約定済み分との二重建玉 = over-fill、review HIGH)。"""
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O2"})
    a = _logged_in_adapter()
    a._register_order(_ref(qty=100.0))
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "ACCEPTED"
    send_req = next(r for r in httpx_mock.get_requests() if str(r.url).endswith("/sendorder"))
    assert json.loads(send_req.content)["Qty"] == 60  # 100 - 40 約定済み
    assert a._orders_ref["C1"].qty == 60.0  # 新しい原数量 = 残数量
    assert a._orders_ref["C1"].order_id == "O2"


async def test_modify_when_already_filled_to_target_skips_resubmit(httpx_mock: HTTPXMock):
    """取消確定までに目標数量まで約定済み (残 0) なら再発注せず終端状態を返す。"""
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 100, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 100, "Price": 2500.0, "State": 3}]}],
    )
    a = _logged_in_adapter()
    a._register_order(_ref(qty=100.0))
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "FILLED"
    assert res.filled_qty == 100.0
    assert "C1" not in a._orders_ref  # 終端化・再発注なし
    # /sendorder は呼ばれない
    assert not any(str(r.url).endswith("/sendorder") for r in httpx_mock.get_requests())


async def test_modify_resubmit_system_error_propagates(httpx_mock: HTTPXMock):
    """再発注が Result=-1 (システムエラー) なら KabuApiError を伝播し (§2.2)、
    原注文は unregister せず polling に後追いさせる。"""
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 0,
               "Details": [{"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": -1})
    a = _logged_in_adapter()
    a._register_order(_ref(qty=100.0))
    with pytest.raises(KabuApiError):
        await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert "C1" in a._orders_ref  # 取消済みだが polling 後追いのため残す
    assert "C1" not in a._modifying  # finally で抑止解除済み


async def test_cancel_then_poll_reconciles_partial_fill(httpx_mock: HTTPXMock):
    """cancel_order は即時 CANCELED(filled=0) を返すが、約定の真実は polling が後追いで
    反映する (review H2: cancel 応答は fill-blind だが poll が CumQty を補正)。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref())
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    cres = await a.cancel_order(venue="KABU", order_id="C1")
    assert cres.status == "CANCELED" and cres.filled_qty == 0.0
    assert "C1" in a._orders_ref  # cancel は unregister しない (polling 継続)
    # 取消成立までに 40 約定していた → poll が CumQty=40 を後追い反映
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert len(events) == 1
    assert events[0].status == "CANCELED"
    assert events[0].filled_qty == 40.0
    assert "C1" not in a._orders_ref  # 終端 → unregister


async def test_poll_loop_backs_off_on_repeated_failure():
    """polling が連続失敗 (本体ログアウト等) したら指数バックオフで間隔を延ばす
    (review M3: 1Hz hot-loop + R5 流量浪費の回避)。"""
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=lambda e: None)
    a._register_order(_ref())  # 追跡注文あり → 自己終了しない

    delays: list[float] = []

    async def _recording_sleep(d: float) -> None:
        delays.append(d)
        if len(delays) >= 4:
            raise asyncio.CancelledError  # ループを止める

    async def _always_fail() -> None:
        raise KabuApiError(401, "logged out")

    a._rate_limit_sleep = _recording_sleep
    a._poll_orders_once = _always_fail  # type: ignore[method-assign]
    await a._run_orders_poll()  # CancelledError は sleep 内 → ループは return する
    # 初回 1s → 失敗ごとに倍 (2,4,...)。単調増加を確認。
    assert delays[0] == 1.0
    assert delays[1] == 2.0
    assert delays[2] == 4.0


# ---------------------------------------------------------------------------
# fetch_account
# ---------------------------------------------------------------------------


async def test_fetch_account_maps_cash_and_positions(httpx_mock: HTTPXMock):
    httpx_mock.add_response(method="GET", url=_WALLET, json={"StockAccountWallet": 1000000.0})
    httpx_mock.add_response(
        method="GET",
        json=[
            {"Symbol": "7203", "LeavesQty": 100, "Price": 2500.0, "ProfitLoss": 5000.0},
            {"Symbol": "6758", "LeavesQty": 0, "Price": 1.0, "ProfitLoss": 0.0},  # 保有ゼロ→除外
        ],
    )
    a = _logged_in_adapter()
    snap = await a.fetch_account()
    assert snap.cash == 1000000.0
    assert snap.buying_power == 1000000.0
    assert len(snap.positions) == 1
    assert snap.positions[0].symbol == "7203"
    assert snap.positions[0].qty == 100
    assert snap.positions[0].avg_price == 2500.0
    assert snap.positions[0].unrealized_pnl == 5000.0


# ---------------------------------------------------------------------------
# GET /orders polling → OrderEvent push
# ---------------------------------------------------------------------------


async def test_poll_pushes_event_and_dedups(httpx_mock: HTTPXMock):
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref())
    httpx_mock.add_response(
        method="GET",
        is_reusable=True,
        json=[{"ID": "O1", "State": 3, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert len(events) == 1
    assert events[0].status == "PARTIALLY_FILLED"
    assert events[0].filled_qty == 40.0
    assert events[0].venue_order_id == "O1"
    assert events[0].client_order_id == "C1"
    # 状態・約定量に変化なし → 再 push しない (dedup)
    await a._poll_orders_once()
    assert len(events) == 1


async def test_poll_skips_orders_not_ours(httpx_mock: HTTPXMock):
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref(order_id="O1"))
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "OTHER", "State": 3, "OrderQty": 100, "CumQty": 10, "Details": []}],
    )
    await a._poll_orders_once()
    assert events == []


async def test_poll_unregisters_terminal_order(httpx_mock: HTTPXMock):
    """終端 (FILLED) を push したら以降ポーリングしない (レジストリから除去)。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref())
    httpx_mock.add_response(
        method="GET",
        is_reusable=True,
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 100, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 100, "Price": 2500.0, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert len(events) == 1 and events[0].status == "FILLED"
    assert "C1" not in a._orders_ref
    assert "O1" not in a._order_id_to_cid
    # 全注文が終端化 → HTTP を叩かない
    n_before = len(httpx_mock.get_requests())
    await a._poll_orders_once()
    assert len(httpx_mock.get_requests()) == n_before


async def test_poll_suppressed_while_modifying(httpx_mock: HTTPXMock):
    """訂正進行中の注文は polling が中間状態を push しない。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref())
    a._modifying.add("C1")
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 0,
               "Details": [{"RecType": 6, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert events == []
    assert "C1" in a._orders_ref  # 訂正中は unregister もしない


async def test_poll_noop_when_no_tracked_orders(httpx_mock: HTTPXMock):
    """追跡注文がなければ HTTP を一切叩かない (idle polling コスト回避)。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    await a._poll_orders_once()
    assert httpx_mock.get_requests() == []
    assert events == []


async def test_poll_loop_self_terminates_when_no_orders():
    """全注文終端 (= _orders_ref 空) で polling ループは自己終了する (idle task を畳む)。"""
    a = _logged_in_adapter()  # _rate_limit_sleep は no-op
    a.set_execution_hooks(secret_resolver=None, on_order_event=lambda e: None)
    # 追跡注文ゼロ → 最初の sleep 後に return するはず (hang しないこと)
    await asyncio.wait_for(a._run_orders_poll(), timeout=1.0)
