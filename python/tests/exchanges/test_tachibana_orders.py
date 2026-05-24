"""Tests for tachibana_orders — Phase 9 Step 5 発注系ペイロード組み立て & 応答パース。

純粋関数のみを検証する (HTTP / secret / WS は adapter 側の責務)。
仕様根拠: order_params.md / マニュアル #CLMKabuNewOrder #CLMKabuCorrectOrder
#CLMKabuCancelOrder。skill R6 (p_errno + sResultCode の 2 段判定)。
"""
from __future__ import annotations

import pytest

from engine.exchanges import tachibana_orders as to
from engine.exchanges.tachibana_auth import ApiError, SessionExpiredError


# ---------------------------------------------------------------------------
# side / time_in_force マッピング
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("side,expected", [("BUY", "3"), ("SELL", "1")])
def test_side_to_baibai_kubun(side, expected):
    assert to.side_to_baibai_kubun(side) == expected


def test_side_to_baibai_kubun_rejects_unknown():
    with pytest.raises(ValueError):
        to.side_to_baibai_kubun("HOLD")


@pytest.mark.parametrize(
    "tif,expected",
    [("DAY", "0"), ("OPENING", "2"), ("CLOSING", "4")],
)
def test_tif_to_condition(tif, expected):
    assert to.tif_to_condition(tif) == expected


def test_tif_to_condition_unknown_defaults_to_zero():
    # 未知 TIF は当日中扱い (sCondition='0') にフォールバックする。
    assert to.tif_to_condition("WEIRD") == "0"


# ---------------------------------------------------------------------------
# CLMKabuNewOrder ペイロード
# ---------------------------------------------------------------------------


def _new_order_kwargs(**overrides):
    base = dict(
        issue_code="7203",
        side="BUY",
        qty=100.0,
        price=None,
        order_type="MARKET",
        time_in_force="DAY",
        second_password="pswd",
        zyoutoeki_kazei_c="1",
    )
    base.update(overrides)
    return base


def test_new_order_sets_clmid_and_required_fields():
    p = to.build_new_order_payload(**_new_order_kwargs())
    assert p["sCLMID"] == "CLMKabuNewOrder"
    assert p["sIssueCode"] == "7203"
    assert p["sSizyouC"] == "00"
    assert p["sBaibaiKubun"] == "3"  # BUY
    assert p["sOrderSuryou"] == "100"
    assert p["sGenkinShinyouKubun"] == "0"  # 現物
    assert p["sZyoutoekiKazeiC"] == "1"  # ログイン応答流用
    assert p["sSecondPassword"] == "pswd"


def test_new_order_market_uses_zero_price():
    p = to.build_new_order_payload(**_new_order_kwargs(order_type="MARKET", price=None))
    assert p["sOrderPrice"] == "0"
    assert p["sCondition"] == "0"


def test_new_order_limit_uses_price_string():
    p = to.build_new_order_payload(
        **_new_order_kwargs(order_type="LIMIT", price=2430.0)
    )
    assert p["sOrderPrice"] == "2430"


def test_new_order_limit_keeps_fractional_price():
    p = to.build_new_order_payload(
        **_new_order_kwargs(order_type="LIMIT", price=2430.5)
    )
    assert p["sOrderPrice"] == "2430.5"


def test_new_order_sell_opening_maps_condition_and_kubun():
    p = to.build_new_order_payload(
        **_new_order_kwargs(side="SELL", time_in_force="OPENING")
    )
    assert p["sBaibaiKubun"] == "1"  # SELL
    assert p["sCondition"] == "2"  # 寄付


def test_new_order_second_password_is_required():
    with pytest.raises(ValueError, match="second_password"):
        to.build_new_order_payload(**_new_order_kwargs(second_password=""))


# ---------------------------------------------------------------------------
# CLMKabuCorrectOrder ペイロード (訂正)
# ---------------------------------------------------------------------------


def test_correct_order_two_identifiers_and_no_change_stars():
    p = to.build_correct_order_payload(
        order_number="9000015",
        eigyou_day="20221209",
        second_password="pswd",
        new_price=None,
        new_qty=None,
    )
    assert p["sCLMID"] == "CLMKabuCorrectOrder"
    assert p["sOrderNumber"] == "9000015"
    assert p["sEigyouDay"] == "20221209"
    # 変更しない項目は "*"
    assert p["sOrderPrice"] == "*"
    assert p["sOrderSuryou"] == "*"
    assert p["sCondition"] == "*"
    assert p["sSecondPassword"] == "pswd"


def test_correct_order_new_price_only():
    p = to.build_correct_order_payload(
        order_number="9000015",
        eigyou_day="20221209",
        second_password="pswd",
        new_price=430.0,
        new_qty=None,
    )
    assert p["sOrderPrice"] == "430"
    assert p["sOrderSuryou"] == "*"


def test_correct_order_new_qty_only():
    p = to.build_correct_order_payload(
        order_number="9000015",
        eigyou_day="20221209",
        second_password="pswd",
        new_price=None,
        new_qty=200.0,
    )
    assert p["sOrderPrice"] == "*"
    assert p["sOrderSuryou"] == "200"


def test_correct_order_requires_second_password():
    with pytest.raises(ValueError, match="second_password"):
        to.build_correct_order_payload(
            order_number="9000015", eigyou_day="20221209",
            second_password="", new_price=430.0,
        )


# ---------------------------------------------------------------------------
# CLMKabuCancelOrder ペイロード (取消)
# ---------------------------------------------------------------------------


def test_cancel_order_two_identifiers_and_password():
    p = to.build_cancel_order_payload(
        order_number="30000007", eigyou_day="20200727", second_password="pswd",
    )
    assert p["sCLMID"] == "CLMKabuCancelOrder"
    assert p["sOrderNumber"] == "30000007"
    assert p["sEigyouDay"] == "20200727"
    assert p["sSecondPassword"] == "pswd"


def test_cancel_order_requires_second_password():
    with pytest.raises(ValueError, match="second_password"):
        to.build_cancel_order_payload(
            order_number="30000007", eigyou_day="20200727", second_password="",
        )


# ---------------------------------------------------------------------------
# parse_order_response — 2 段階エラー判定 (R6)
# ---------------------------------------------------------------------------


def test_parse_order_response_success_extracts_ids():
    payload = {
        "sCLMID": "CLMKabuNewOrder", "p_errno": "0", "sResultCode": "0",
        "sResultText": "", "sOrderNumber": "9000015", "sEigyouDay": "20221209",
        "sOrderDate": "20221209134803",
    }
    ack = to.parse_order_response(payload)
    assert ack.rejected is False
    assert ack.order_number == "9000015"
    assert ack.eigyou_day == "20221209"
    assert ack.order_date == "20221209134803"


def test_parse_order_response_empty_p_errno_is_ok():
    # R6: p_errno は空文字でも正常扱い。
    payload = {
        "p_errno": "", "sResultCode": "0",
        "sOrderNumber": "1", "sEigyouDay": "20260101",
    }
    ack = to.parse_order_response(payload)
    assert ack.rejected is False
    assert ack.order_number == "1"


def test_parse_order_response_business_rejection_does_not_raise():
    # sResultCode != 0 (例: 21=余力不足) は業務リジェクト。例外ではなく rejected=True。
    payload = {
        "p_errno": "0", "sResultCode": "21", "sResultText": "可能額不足",
    }
    ack = to.parse_order_response(payload)
    assert ack.rejected is True
    assert ack.reject_code == "21"
    assert ack.reject_text == "可能額不足"


def test_parse_order_response_session_expired_raises():
    # p_errno='2' は仮想URL無効 → 再ログイン要 (connection-level、raise)。
    payload = {"p_errno": "2", "sResultCode": "0"}
    with pytest.raises(SessionExpiredError):
        to.parse_order_response(payload)


def test_parse_order_response_connection_error_raises():
    payload = {"p_errno": "-1", "sResultCode": "0", "p_err": "boom"}
    with pytest.raises(ApiError):
        to.parse_order_response(payload)


# ---------------------------------------------------------------------------
# parse_ec_frame / ec_status — 約定通知 (情報コードは e-station 参照で確定)
# ---------------------------------------------------------------------------


def _ec_fields(**overrides):
    base = {
        to._EC_ORDER_NUMBER: "9000015",   # p_NO
        to._EC_TRADE_ID: "1",             # p_EDA
        to._EC_NOTIFY_TYPE: "2",          # p_NT 約定
        to._EC_LAST_PRICE: "2430",        # p_DH
        to._EC_LAST_QTY: "100",           # p_DSU
        to._EC_LEAVES_QTY: "0",           # p_ZSU 全約定
        to._EC_EXEC_DATETIME: "20260521134803",  # p_OD JST
    }
    base.update(overrides)
    return base


def test_parse_ec_frame_extracts_fields():
    rep = to.parse_ec_frame(_ec_fields())
    assert rep is not None
    assert rep.venue_order_id == "9000015"
    assert rep.trade_id == "1"
    assert rep.notification_type == "2"
    assert rep.last_price == 2430.0
    assert rep.last_qty == 100.0
    assert rep.leaves_qty == 0.0  # "0" は有効値 (全約定)
    # p_OD (JST YYYYMMDDHHMMSS) → UTC ms。2026-05-21 13:48:03 JST = 04:48:03 UTC。
    from datetime import datetime, timezone, timedelta
    expected = int(datetime(2026, 5, 21, 13, 48, 3, tzinfo=timezone(timedelta(hours=9))).timestamp() * 1000)
    assert rep.ts_event_ms == expected


def test_parse_ec_frame_canceled_has_no_price_qty():
    rep = to.parse_ec_frame({to._EC_ORDER_NUMBER: "9000015", to._EC_NOTIFY_TYPE: "3"})
    assert rep.notification_type == "3"
    assert rep.last_price is None
    assert rep.last_qty is None
    assert rep.leaves_qty is None


def test_parse_ec_frame_without_order_number_is_ignored():
    assert to.parse_ec_frame({to._EC_NOTIFY_TYPE: "2"}) is None


def test_parse_ec_frame_bad_datetime_is_zero():
    rep = to.parse_ec_frame(_ec_fields(**{to._EC_EXEC_DATETIME: "garbage"}))
    assert rep.ts_event_ms == 0


@pytest.mark.parametrize(
    "nt,leaves,expected",
    [
        ("1", None, "ACCEPTED"),      # 受付
        ("2", 0.0, "FILLED"),         # 約定・残0
        ("2", 50.0, "PARTIALLY_FILLED"),  # 約定・残あり
        ("2", None, "FILLED"),        # 約定・残不明 → 全約定扱い
        ("3", None, "CANCELED"),      # 取消
        ("4", None, "EXPIRED"),       # 失効
        ("99", None, "ACCEPTED"),     # 未知種別
    ],
)
def test_ec_status_mapping(nt, leaves, expected):
    assert to.ec_status(nt, leaves) == expected


# ---------------------------------------------------------------------------
# CLMOrderList — build_order_list_payload / parse_order_list_response (Slice 3b)
# ---------------------------------------------------------------------------


def _order_list_item(**overrides) -> dict:
    """CLMOrderList aOrderList の最小レコード (BUY LIMIT 7203.TSE, sOrderSyoukaiStatus=5)。"""
    base = {
        "sOrderOrderNumber": "12345",
        "sOrderIssueCode": "7203",
        "sOrderSizyouC": "00",
        "sOrderBaibaiKubun": "3",  # 買
        "sOrderOrderSuryou": "100",
        "sOrderCurrentSuryou": "100",
        "sOrderOrderPrice": "2430",
        "sOrderOrderPriceKubun": "2",  # 指値
        "sOrderStatusCode": "0",
    }
    base.update(overrides)
    return base


def test_build_order_list_payload_targets_working_orders():
    payload = to.build_order_list_payload()
    assert payload["sCLMID"] == "CLMOrderList"
    assert payload["sOrderSyoukaiStatus"] == "5"  # 未約定+一部約定


def test_parse_order_list_response_buy_limit():
    resp = {"aOrderList": [_order_list_item()], "p_errno": "0", "sResultCode": "0"}
    rows = to.parse_order_list_response(resp)
    assert len(rows) == 1
    row = rows[0]
    assert row.venue_order_id == "12345"
    assert row.issue_code == "7203"
    assert row.sizyou_c == "00"
    assert row.side == "BUY"
    assert row.qty == 100.0
    assert row.price == 2430.0


def test_parse_order_list_response_sell_market():
    item = _order_list_item(
        sOrderBaibaiKubun="1",   # 売
        sOrderOrderPrice="0",
        sOrderOrderPriceKubun="1",  # 成行
    )
    resp = {"aOrderList": [item], "p_errno": "0", "sResultCode": "0"}
    rows = to.parse_order_list_response(resp)
    assert len(rows) == 1
    row = rows[0]
    assert row.side == "SELL"
    assert row.price is None


def test_parse_order_list_response_empty_list():
    resp = {"aOrderList": "", "p_errno": "0", "sResultCode": "0"}
    assert to.parse_order_list_response(resp) == []


def test_parse_order_list_response_multiple_orders():
    items = [
        _order_list_item(sOrderOrderNumber="1001"),
        _order_list_item(sOrderOrderNumber="1002", sOrderBaibaiKubun="1"),
    ]
    resp = {"aOrderList": items, "p_errno": "0", "sResultCode": "0"}
    rows = to.parse_order_list_response(resp)
    assert len(rows) == 2
    assert rows[0].venue_order_id == "1001"
    assert rows[1].side == "SELL"
