"""Phase 9 Step 6: kabusapi_orders 純粋関数のテスト。"""

import pytest

from engine.exchanges import kabusapi_orders as o


# ---------------------------------------------------------------------------
# side_to_kabu / front_order_type
# ---------------------------------------------------------------------------


def test_side_to_kabu_buy_sell():
    assert o.side_to_kabu("BUY") == "2"
    assert o.side_to_kabu("SELL") == "1"
    assert o.side_to_kabu("buy") == "2"  # case-insensitive


def test_side_to_kabu_unknown_raises():
    with pytest.raises(ValueError):
        o.side_to_kabu("HOLD")


@pytest.mark.parametrize(
    "order_type,tif,expected",
    [
        ("MARKET", "DAY", 10),
        ("LIMIT", "DAY", 20),
        ("MARKET", "OPENING", 13),
        ("LIMIT", "OPENING", 21),
        ("MARKET", "CLOSING", 16),
        ("LIMIT", "CLOSING", 24),
        ("MARKET", "UNKNOWN", 10),  # 未知 TIF は当日中扱い
    ],
)
def test_front_order_type(order_type, tif, expected):
    assert o.front_order_type(order_type, tif) == expected


# ---------------------------------------------------------------------------
# build_send_order_payload
# ---------------------------------------------------------------------------


def test_build_send_order_buy_limit_fields():
    p = o.build_send_order_payload(
        symbol="7203", exchange=1, side="BUY", qty=100, price=2500.0,
        order_type="LIMIT", time_in_force="DAY",
    )
    assert p["Symbol"] == "7203"
    assert p["Side"] == "2"  # 買
    assert p["CashMargin"] == 1  # 現物
    assert p["SecurityType"] == 1
    assert p["DelivType"] == 2  # 現物買 = お預り金
    assert p["FundType"] == "AA"  # 信用代用
    assert p["FrontOrderType"] == 20  # 指値
    assert p["Price"] == 2500.0
    assert p["Qty"] == 100
    assert p["ExpireDay"] == 0


def test_build_send_order_sell_market_fields():
    p = o.build_send_order_payload(
        symbol="7203", exchange=1, side="SELL", qty=300, price=None,
        order_type="MARKET", time_in_force="DAY",
    )
    assert p["Side"] == "1"  # 売
    assert p["DelivType"] == 0  # 現物売 = 指定なし
    assert p["FundType"] == "  "  # 半角スペース2つ
    assert p["FrontOrderType"] == 10  # 成行
    assert p["Price"] == 0  # 成行は 0


def test_build_send_order_has_no_password_field():
    """R3: kabu 発注に Password フィールドは存在しない (Tachibana 第二暗証番号と違う)。"""
    p = o.build_send_order_payload(
        symbol="7203", exchange=1, side="BUY", qty=100, price=2500.0,
        order_type="LIMIT", time_in_force="DAY",
    )
    assert "Password" not in p
    assert "sSecondPassword" not in p


def test_build_send_order_limit_without_price_raises():
    with pytest.raises(ValueError, match="LIMIT order requires a price"):
        o.build_send_order_payload(
            symbol="7203", exchange=1, side="BUY", qty=100, price=None,
            order_type="LIMIT", time_in_force="DAY",
        )


def test_build_send_order_unknown_type_raises():
    with pytest.raises(ValueError, match="unknown order_type"):
        o.build_send_order_payload(
            symbol="7203", exchange=1, side="BUY", qty=100, price=1.0,
            order_type="STOP", time_in_force="DAY",
        )


def test_build_send_order_account_type_override():
    p = o.build_send_order_payload(
        symbol="7203", exchange=1, side="BUY", qty=100, price=2500.0,
        order_type="LIMIT", time_in_force="DAY", account_type=2,
    )
    assert p["AccountType"] == 2


# ---------------------------------------------------------------------------
# build_cancel_order_payload
# ---------------------------------------------------------------------------


def test_build_cancel_order_only_order_id():
    """R3: 取消は OrderID のみ。Password 不要。"""
    p = o.build_cancel_order_payload(order_id="20200709A02N04712032")
    assert p == {"OrderID": "20200709A02N04712032"}


def test_build_cancel_order_empty_raises():
    with pytest.raises(ValueError):
        o.build_cancel_order_payload(order_id="")


# ---------------------------------------------------------------------------
# parse_send_order_response
# ---------------------------------------------------------------------------


def test_parse_send_order_success():
    ack = o.parse_send_order_response({"Result": 0, "OrderId": "OID1"})
    assert ack.rejected is False
    assert ack.order_id == "OID1"


def test_parse_send_order_business_reject():
    ack = o.parse_send_order_response({"Result": 21, "Message": "余力不足"})
    assert ack.rejected is True
    assert ack.reject_code == "21"
    assert ack.reject_text == "余力不足"
    assert ack.order_id == ""


def test_parse_send_order_system_error_reject():
    ack = o.parse_send_order_response({"Result": -1})
    assert ack.rejected is True
    assert ack.reject_code == "-1"


def test_parse_send_order_missing_result_is_success():
    """Result 欠落は 0 (success) 扱い。"""
    ack = o.parse_send_order_response({"OrderId": "OID9"})
    assert ack.rejected is False
    assert ack.order_id == "OID9"


# ---------------------------------------------------------------------------
# parse_order_status / order_status
# ---------------------------------------------------------------------------


def test_order_status_filled():
    assert o.order_status(state=5, order_qty=100, cum_qty=100, details=[]) == "FILLED"


def test_order_status_partial_then_terminal_is_canceled():
    assert o.order_status(state=5, order_qty=100, cum_qty=40, details=[]) == "CANCELED"


def test_order_status_terminal_zero_fill_canceled():
    details = [{"RecType": 6, "State": 3}]  # 取消
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=details) == "CANCELED"


def test_order_status_terminal_zero_fill_expired():
    details = [{"RecType": 7, "State": 3}]  # 失効
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=details) == "EXPIRED"


def test_order_status_terminal_zero_fill_rejected():
    details = [{"RecType": 4, "State": 4}]  # 発注エラー (取消/失効でない)
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=details) == "REJECTED"


def test_order_status_accepted_when_open_no_fill():
    assert o.order_status(state=3, order_qty=100, cum_qty=0, details=[]) == "ACCEPTED"


def test_order_status_partially_filled_when_open():
    assert o.order_status(state=3, order_qty=100, cum_qty=40, details=[]) == "PARTIALLY_FILLED"


def test_parse_order_status_full_record():
    report = o.parse_order_status(
        {
            "ID": "OID1",
            "State": 5,
            "OrderQty": 100,
            "CumQty": 100,
            "Price": 2500.0,
            "Details": [
                {"RecType": 8, "Qty": 60, "Price": 2500.0, "State": 3, "TransactTime": "20260521101500"},
                {"RecType": 8, "Qty": 40, "Price": 2510.0, "State": 3, "TransactTime": "20260521101600"},
            ],
        }
    )
    assert report is not None
    assert report.order_id == "OID1"
    assert report.status == "FILLED"
    assert report.filled_qty == 100.0
    assert report.terminal is True
    # 数量加重平均: (60*2500 + 40*2510) / 100 = 2504.0
    assert report.avg_price == pytest.approx(2504.0)
    assert report.ts_ms > 0


def test_parse_order_status_no_id_returns_none():
    assert o.parse_order_status({"State": 5}) is None


def test_parse_order_status_uses_price_fallback_when_no_exec_details():
    report = o.parse_order_status(
        {"ID": "OID2", "State": 3, "OrderQty": 100, "CumQty": 0, "Price": 1234.0, "Details": []}
    )
    assert report is not None
    assert report.status == "ACCEPTED"
    assert report.avg_price == 1234.0


def test_order_status_partial_then_expired_is_expired():
    """部分約定して残りが失効 → EXPIRED (CANCELED に丸めない、review M4)。"""
    details = [
        {"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},  # 約定 40
        {"RecType": 7, "State": 3},  # 残り失効
    ]
    assert o.order_status(state=5, order_qty=100, cum_qty=40, details=details) == "EXPIRED"


def test_order_status_partial_then_canceled_via_details_is_canceled():
    """部分約定して残りが取消 → CANCELED。取消明細が失効明細より優先。"""
    details = [
        {"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
        {"RecType": 6, "State": 3},  # 残り取消
        {"RecType": 7, "State": 3},  # 失効明細も混在 → 取消優先
    ]
    assert o.order_status(state=5, order_qty=100, cum_qty=40, details=details) == "CANCELED"


def test_order_status_zero_fill_cancel_detail_state3_is_canceled():
    """取消明細が確定 (State=3 処理済) なら CANCELED に分類する。

    OpenAPI Details.State enum: 1=待機 / 2=処理中 / 3=処理済 (取消済・全約定・期限切れを
    含む唯一の完了状態) / 4=エラー / 5=削除済み。確定 = 処理済 (3) のみ (review fix #1)。
    """
    details = [{"RecType": 6, "State": 3}]
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=details) == "CANCELED"


def test_order_status_cancel_detail_waiting_or_error_not_confirmed_cancel():
    """fix #1: State==1 (待機) / ==4 (エラー) の取消明細は確定 (処理済=3) ではないので
    CANCELED と誤分類しない。約定ゼロ・取消明細が非確定なら既定の REJECTED に落ちる
    (取消の確定理由として扱わない)。"""
    waiting = [{"RecType": 6, "State": 1}]
    errored = [{"RecType": 6, "State": 4}]
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=waiting) == "REJECTED"
    assert o.order_status(state=5, order_qty=100, cum_qty=0, details=errored) == "REJECTED"


def test_order_status_state5_parent_nonconfirmed_cancel_detail_not_canceled():
    """fix #1 regression: State==5 (削除済み) の親注文 + CumQty>0 + RecType==6 (取消) 明細が
    Details.State==1 (待機) および ==4 (エラー) のとき、確定取消として分類しない。

    部分約定して終了 (cum_qty>0) の残数量理由判定でも、非確定 (≠3) の取消明細は
    無視され、既定の CANCELED (部分約定後の残数量) に落ちる (EXPIRED にもならない)。"""
    waiting = [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
               {"RecType": 6, "State": 1}]
    errored = [{"RecType": 8, "Qty": 40, "Price": 2500.0, "State": 3},
               {"RecType": 6, "State": 4}]
    # 取消明細が非確定でも残数量の既定は CANCELED (部分約定済みは REJECTED にしない)。
    assert o.order_status(state=5, order_qty=100, cum_qty=40, details=waiting) == "CANCELED"
    assert o.order_status(state=5, order_qty=100, cum_qty=40, details=errored) == "CANCELED"


def test_avg_fill_price_ignores_nonconfirmed_execution_rows():
    """fix #2: RecType==8 (約定) 明細でも Details.State!=3 (処理中=2 / 待機=1 / エラー=4) は
    確定前なので加重平均価格に算入しない (#1 の確定判定と整合)。"""
    # 確定約定 60@2500 のみ採用。State==2 の 40@9999 (処理中) は無視。
    details = [
        {"RecType": 8, "Qty": 60, "Price": 2500.0, "State": 3},
        {"RecType": 8, "Qty": 40, "Price": 9999.0, "State": 2},
    ]
    assert o._avg_fill_price(details, fallback=0.0) == pytest.approx(2500.0)


def test_avg_fill_price_falls_back_when_only_nonconfirmed_executions():
    """確定約定が無く処理中明細のみなら fallback (注文 Price) を返す。"""
    details = [{"RecType": 8, "Qty": 40, "Price": 9999.0, "State": 2}]
    assert o._avg_fill_price(details, fallback=1234.0) == 1234.0


@pytest.mark.parametrize("bad_state", [0, 9, 99])
def test_parse_order_status_invalid_state_returns_none(bad_state):
    """欠損/範囲外 State を 0→ACCEPTED と誤魔化さず行をスキップする (review M5:
    誤 ACCEPTED は終端検知漏れ = レジストリ leak + 無限 polling を招く)。"""
    assert o.parse_order_status({"ID": "X", "State": bad_state}) is None


def test_parse_order_status_missing_state_returns_none():
    assert o.parse_order_status({"ID": "X", "CumQty": 0}) is None
