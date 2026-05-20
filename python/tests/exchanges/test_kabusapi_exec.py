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


async def test_modify_carries_filled_baseline_so_remainder_poll_reports_cumulative(
    httpx_mock: HTTPXMock,
):
    """訂正前に 40/100 約定 → 残 60 を O2 で再発注。in-place remap で O1 を捨てるが、
    約定済み 40 は ref.filled_base に退避し、後続の O2 polling は論理注文の累計約定
    (40 + O2 の CumQty) を報告する。これがないと約定済み 40 が OrderEvent stream から
    永久に消えて filled_qty が過少報告される (review HIGH / in-place remap の裏面)。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref(qty=100.0))
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(  # 取消確定時点で 40@2500 約定済み
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O2"})
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "ACCEPTED"
    # 約定済み 40 を carry → facade/UI の filled_qty が 0 に巻き戻らない。
    assert res.filled_qty == 40.0
    assert res.avg_price == 2500.0
    ref = a._orders_ref["C1"]
    assert ref.filled_base == 40.0
    assert ref.qty == 60.0  # 残数量
    assert ref.order_id == "O2"

    # O2 が残 60 を 2600 で約定 → poll は累計 100 (40+60)・加重平均 2560 を報告する。
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O2", "State": 5, "OrderQty": 60, "CumQty": 60, "Price": 2600.0,
               "Details": [{"RecType": 8, "Qty": 60, "Price": 2600.0, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert len(events) == 1
    assert events[0].status == "FILLED"
    assert events[0].filled_qty == 100.0  # 40 (O1) + 60 (O2) — 約定が消えない
    assert events[0].avg_price == 2560.0  # (40*2500 + 60*2600)/100


async def test_poll_after_modify_reports_partial_when_baseline_but_remainder_unfilled(
    httpx_mock: HTTPXMock,
):
    """remap 後 O2 が未約定 (ACCEPTED) でも、約定済みベースラインがあるなら論理状態は
    PARTIALLY_FILLED として報告する (ACCEPTED に戻すと UI が「約定ゼロの新規」に見える)。"""
    events = []
    a = _logged_in_adapter()
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref(qty=100.0))
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O2"})
    await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)

    httpx_mock.add_response(  # O2 は受付済み・未約定
        method="GET",
        json=[{"ID": "O2", "State": 3, "OrderQty": 60, "CumQty": 0, "Price": 2600.0,
               "Details": []}],
    )
    await a._poll_orders_once()
    assert len(events) == 1
    assert events[0].status == "PARTIALLY_FILLED"
    assert events[0].filled_qty == 40.0


async def test_modify_new_failed_after_partial_reports_filled(httpx_mock: HTTPXMock):
    """取消成功+新規業務リジェクトでも、取消確定までに約定していた数量を CANCELED
    OrderResult の filled_qty に載せる (in-place remap で O1 を捨てるため polling は
    後追いできない → ここで載せないと約定済みが消える)。"""
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 21, "Message": "no bp"})
    a = _logged_in_adapter()
    a._register_order(_ref(qty=100.0))
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "CANCELED"
    assert res.filled_qty == 40.0
    assert res.avg_price == 2500.0
    assert "C1" not in a._orders_ref


async def test_cancel_order_rejected_while_modifying():
    """訂正進行中の注文に対する取消は MODIFY_IN_PROGRESS で弾く (cancel↔modify の
    re-entrancy で remap 後の live 注文を孤児化させない)。"""
    a = _logged_in_adapter()
    a._register_order(_ref())
    a._modifying.add("C1")
    res = await a.cancel_order(venue="KABU", order_id="C1")
    assert res.status == "REJECTED"
    assert res.reject_reason == "MODIFY_IN_PROGRESS"


async def test_modify_order_rejected_while_modifying():
    """訂正進行中の注文への多重訂正は MODIFY_IN_PROGRESS で弾く (二本目の finally が
    suppression window を先に畳む re-entrancy を防ぐ。cancel ガードと対称)。"""
    a = _logged_in_adapter()
    a._register_order(_ref())
    a._modifying.add("C1")
    res = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res.status == "REJECTED"
    assert res.reject_reason == "MODIFY_IN_PROGRESS"


async def test_double_modify_telescopes_filled_baseline(httpx_mock: HTTPXMock):
    """連続訂正で filled_base が累積し、論理注文の総目標数量 (filled_base + qty) が
    保たれることを固定する。100 → 40 約定 → 訂正 → O2 が 20 約定 → 再訂正 →
    残 40 を O3 で発注 → O3 全約定で累計 100 FILLED。"""
    a = _logged_in_adapter()
    events = []
    a.set_execution_hooks(secret_resolver=None, on_order_event=events.append)
    a._register_order(_ref(qty=100.0))

    # --- 1 回目の訂正: O1 が 40@2500 約定済みで取消 → 残 60 を O2 で発注 ---
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O1"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O1", "State": 5, "OrderQty": 100, "CumQty": 40, "Price": 2500.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O2"})
    res1 = await a.modify_order(venue="KABU", order_id="C1", new_price=2600.0)
    assert res1.status == "ACCEPTED"
    ref = a._orders_ref["C1"]
    assert ref.filled_base == 40.0 and ref.qty == 60.0  # 総目標 = 40 + 60 = 100

    # --- 2 回目の訂正: O2 が 20@2600 約定済みで取消 → 残 40 を O3 で発注 ---
    httpx_mock.add_response(method="PUT", url=_CANCEL, json={"Result": 0, "OrderId": "O2"})
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O2", "State": 5, "OrderQty": 60, "CumQty": 20, "Price": 2600.0,
               "Details": [{"RecType": 8, "Qty": 20, "Price": 2600.0, "State": 3},
                           {"RecType": 6, "State": 3}]}],
    )
    httpx_mock.add_response(method="POST", url=_SEND, json={"Result": 0, "OrderId": "O3"})
    res2 = await a.modify_order(venue="KABU", order_id="C1", new_price=2700.0)
    assert res2.status == "ACCEPTED"
    assert res2.filled_qty == 60.0  # 累計約定 40 + 20
    ref = a._orders_ref["C1"]
    assert ref.filled_base == 60.0 and ref.qty == 40.0  # 総目標 = 60 + 40 = 100 を維持
    assert ref.order_id == "O3"
    # 残 40 を再発注 (= 100 - 60 約定済み、二重建玉なし)。
    last_send = [r for r in httpx_mock.get_requests() if str(r.url).endswith("/sendorder")][-1]
    assert json.loads(last_send.content)["Qty"] == 40

    # --- O3 が残 40 を 2700 で約定 → poll は累計 100 FILLED を報告 ---
    events.clear()
    httpx_mock.add_response(
        method="GET",
        json=[{"ID": "O3", "State": 5, "OrderQty": 40, "CumQty": 40, "Price": 2700.0,
               "Details": [{"RecType": 8, "Qty": 40, "Price": 2700.0, "State": 3}]}],
    )
    await a._poll_orders_once()
    assert len(events) == 1
    assert events[0].status == "FILLED"
    assert events[0].filled_qty == 100.0  # 40 (O1) + 20 (O2) + 40 (O3)
    # 加重平均 = (40*2500 + 20*2600 + 40*2700)/100 = 2600
    assert events[0].avg_price == 2600.0


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


# ---------------------------------------------------------------------------
# check_health (Phase 9 Step 7: Venue Health Watchdog, §3.5)
# ---------------------------------------------------------------------------

_APISOFTLIMIT = endpoint("apisoftlimit", env="verify")


def test_set_execution_hooks_accepts_on_venue_logout_kwarg():
    """server_grpc は on_venue_logout も注入する。kabu は poll 型 watchdog で検知するため
    受理して無視する (Tachibana だけが SS push でこの hook を使う)。"""
    a = KabuStationAdapter()
    a.set_execution_hooks(
        secret_resolver=None, on_order_event=lambda e: None, on_venue_logout=lambda v: None
    )
    assert a._on_order_event is not None


async def test_check_health_logged_in_returns_true(httpx_mock: HTTPXMock):
    """本体ログイン中: GET /apisoftlimit が Code=0 → True。"""
    httpx_mock.add_response(
        method="GET", url=_APISOFTLIMIT,
        json={"Code": 0, "SoftLimit": {"Stock": 200, "Margin": 200}},
    )
    a = _logged_in_adapter()
    assert await a.check_health() is True


async def test_check_health_4001007_returns_false(httpx_mock: HTTPXMock):
    """本体ログアウト (4001007 ログイン認証エラー) → False (例外にしない)。"""
    httpx_mock.add_response(
        method="GET", url=_APISOFTLIMIT,
        json={"Code": 4001007, "Message": "ログイン認証エラー"},
    )
    a = _logged_in_adapter()
    assert await a.check_health() is False


async def test_check_health_4001017_http401_returns_false(httpx_mock: HTTPXMock):
    """本体未ログイン (4001017) が HTTP 401 で来ても False に正規化する。"""
    httpx_mock.add_response(
        method="GET", url=_APISOFTLIMIT, status_code=401,
        json={"Code": 4001017, "Message": "ログイン認証エラー"},
    )
    a = _logged_in_adapter()
    assert await a.check_health() is False


async def test_check_health_transient_error_propagates(httpx_mock: HTTPXMock):
    """ログアウト以外のエラー (流量 429 等) は transient として伝播する (watchdog が握る)。
    誤って False を返すと spurious な再ログイン modal が出るため、必ず raise する。"""
    httpx_mock.add_response(
        method="GET", url=_APISOFTLIMIT, status_code=429,
        json={"Code": 4002006, "Message": "スロットリング制限エラー"},
    )
    a = _logged_in_adapter()
    with pytest.raises(KabuApiError):
        await a.check_health()


async def test_check_health_requires_login():
    """未ログイン (token なし) は RuntimeError (transient 扱い)。teardown race で
    spurious なログアウト検出をしないため False ではなく raise する。"""
    a = KabuStationAdapter(environment="verify")
    with pytest.raises(RuntimeError):
        await a.check_health()
