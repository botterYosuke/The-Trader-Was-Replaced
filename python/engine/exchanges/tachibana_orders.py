"""Tachibana 発注系 sCLMID のペイロード組み立て & 応答パース (Phase 9 Step 5)。

純粋関数のみ (I/O なし)。HTTP 送信・第二暗証番号の解決・EVENT WS は
``TachibanaAdapter`` / ``secret_provider`` の責務であり本モジュールには持ち込まない
(tachibana skill: 立花 I/O は Python 側に集約しつつ、組み立てロジックは純粋関数で
テスト可能にする方針)。

仕様根拠:
- order_params.md / マニュアル ``#CLMKabuNewOrder`` ``#CLMKabuCorrectOrder``
  ``#CLMKabuCancelOrder``
- ``sSecondPassword`` は新規 / 訂正 / 取消すべてで必須 (計画 §0.1、skill 確認済み)。
  ブラウザ版と異なり API 発注では省略不可。
- 訂正・取消は ``sOrderNumber`` + ``sEigyouDay`` の 2 識別子が必須
  (注文番号は営業日と組でユニーク、マニュアル応答仕様)。
- エラーは R6 の 2 段階判定: ``p_errno`` (接続/認証レベル) → ``sResultCode``
  (業務レベル)。接続レベル異常は raise、業務リジェクト (余力不足等) は
  ``OrderAck(rejected=True)`` で返し、呼び出し側が REJECTED 注文として扱う。
"""
from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone

from engine.exchanges.tachibana_auth import check_response

log = logging.getLogger(__name__)

# JST (約定日時 p_OD は JST)。
_JST = timezone(timedelta(hours=9))

# sBaibaiKubun (売買区分): 1=売 / 3=買 (現物。現渡 5 / 現引 7 は Phase 9 非対象)。
_SIDE_TO_BAIBAI: dict[str, str] = {"BUY": "3", "SELL": "1"}

# sCondition (執行条件): 0=指定なし(当日中) / 2=寄付 / 4=引け / 6=不成。
# OrderPanel が送る time_in_force は DAY / OPENING / CLOSING の 3 値のみ
# (src/ui/order_panel.rs TimeInForce::wire)。不成は現状 UI から発射されない。
_TIF_TO_CONDITION: dict[str, str] = {"DAY": "0", "OPENING": "2", "CLOSING": "4"}

# "変更しない" を表す Tachibana の予約値 (訂正系の各項目)。
_NO_CHANGE = "*"


@dataclass(frozen=True)
class OrderAck:
    """発注系応答の正規化結果。

    ``rejected`` が True のときは業務リジェクト (sResultCode != 0)。注文 ID は
    採番されない。``rejected`` が False のときは ``order_number`` / ``eigyou_day``
    が有効 (新規/訂正/取消すべて応答に含む)。
    """

    rejected: bool
    order_number: str = ""
    eigyou_day: str = ""
    order_date: str = ""
    reject_code: str = ""
    reject_text: str = ""


def side_to_baibai_kubun(side: str) -> str:
    """BUY/SELL → sBaibaiKubun (3/1)。未知 side は ValueError。"""
    try:
        return _SIDE_TO_BAIBAI[side.upper()]
    except KeyError as exc:
        raise ValueError(f"unknown side: {side!r}") from exc


def tif_to_condition(time_in_force: str) -> str:
    """time_in_force → sCondition。未知値は当日中 (0) にフォールバックする。"""
    return _TIF_TO_CONDITION.get(time_in_force.upper(), "0")


def _fmt_price(price: float) -> str:
    """指値価格を Tachibana の sOrderPrice 文字列に整形する。

    整数値は小数点を落とし ("2430")、端数があれば残す ("2430.5")。
    呼値整合 (tick 丸め) は OrderPanel / 呼値マスタの責務でここでは行わない。
    """
    if price == int(price):
        return str(int(price))
    return f"{price}"


def _fmt_qty(qty: float) -> str:
    """注文数量を sOrderSuryou 文字列に整形する (株は整数単位)。"""
    return str(int(qty))


def build_new_order_payload(
    *,
    issue_code: str,
    side: str,
    qty: float,
    price: float | None,
    order_type: str,
    time_in_force: str,
    second_password: str,
    zyoutoeki_kazei_c: str,
    sizyou_c: str = "00",
) -> dict[str, str]:
    """CLMKabuNewOrder (現物新規) のリクエスト dict を組み立てる。

    ``zyoutoeki_kazei_c`` はログイン応答 (sZyoutoekiKazeiC) をそのまま流用する
    のが定石 (order_params.md)。MARKET は sOrderPrice='0'、LIMIT は価格文字列。
    """
    if not second_password:
        raise ValueError("second_password is required for CLMKabuNewOrder")
    order_type_n = order_type.upper()
    if order_type_n == "MARKET":
        order_price = "0"
    elif order_type_n == "LIMIT":
        if price is None:
            raise ValueError("LIMIT order requires a price")
        order_price = _fmt_price(price)
    else:
        raise ValueError(f"unknown order_type: {order_type!r}")

    return {
        "sCLMID": "CLMKabuNewOrder",
        "sIssueCode": issue_code,
        "sSizyouC": sizyou_c,
        "sBaibaiKubun": side_to_baibai_kubun(side),
        "sCondition": tif_to_condition(time_in_force),
        "sOrderPrice": order_price,
        "sOrderSuryou": _fmt_qty(qty),
        "sGenkinShinyouKubun": "0",  # 現物 (Phase 9 は現物のみ)
        "sOrderExpireDay": "0",  # 当日 (期日指定は Phase 9 非対象)
        "sGyakusasiOrderType": "0",  # 通常 (逆指値は非対象)
        "sGyakusasiZyouken": "0",
        "sGyakusasiPrice": _NO_CHANGE,
        "sTatebiType": _NO_CHANGE,  # 現物は建日種類なし
        "sZyoutoekiKazeiC": zyoutoeki_kazei_c,
        "sTategyokuZyoutoekiKazeiC": _NO_CHANGE,  # 現引/現渡時のみ
        "sSecondPassword": second_password,
    }


def build_correct_order_payload(
    *,
    order_number: str,
    eigyou_day: str,
    second_password: str,
    new_price: float | None = None,
    new_qty: float | None = None,
) -> dict[str, str]:
    """CLMKabuCorrectOrder (訂正) のリクエスト dict を組み立てる。

    変更しない項目は ``"*"`` を送る (マニュアル仕様)。価格・数量のいずれか/両方
    のみ可変。sOrderNumber + sEigyouDay の 2 識別子で対象注文を指定する。
    """
    if not second_password:
        raise ValueError("second_password is required for CLMKabuCorrectOrder")
    return {
        "sCLMID": "CLMKabuCorrectOrder",
        "sOrderNumber": order_number,
        "sEigyouDay": eigyou_day,
        "sCondition": _NO_CHANGE,
        "sOrderPrice": _fmt_price(new_price) if new_price is not None else _NO_CHANGE,
        "sOrderSuryou": _fmt_qty(new_qty) if new_qty is not None else _NO_CHANGE,
        "sOrderExpireDay": _NO_CHANGE,
        "sGyakusasiZyouken": _NO_CHANGE,
        "sGyakusasiPrice": _NO_CHANGE,
        "sSecondPassword": second_password,
    }


def build_cancel_order_payload(
    *,
    order_number: str,
    eigyou_day: str,
    second_password: str,
) -> dict[str, str]:
    """CLMKabuCancelOrder (取消) のリクエスト dict を組み立てる。

    Tachibana は取消でも sSecondPassword が必須 (マニュアル確認済み。「取消は
    本人確認済みで不要」という旧記述は誤り、計画 §2.1)。
    """
    if not second_password:
        raise ValueError("second_password is required for CLMKabuCancelOrder")
    return {
        "sCLMID": "CLMKabuCancelOrder",
        "sOrderNumber": order_number,
        "sEigyouDay": eigyou_day,
        "sSecondPassword": second_password,
    }


def parse_order_response(payload: dict) -> OrderAck:
    """発注系応答を R6 の 2 段階で判定して正規化する。

    - ``p_errno`` が空/"0" 以外: 接続/認証レベル異常 → ``check_response`` が
      ``SessionExpiredError`` / ``ApiError`` を raise する。
    - ``p_errno`` 正常かつ ``sResultCode`` != "0"/"" : 業務リジェクト
      (例 余力不足) → ``OrderAck(rejected=True)``。例外にはしない
      (REJECTED 注文として UI に反映するため)。
    - 両方正常: ``OrderAck(rejected=False)`` + 注文 ID。
    """
    p_errno = str(payload.get("p_errno", ""))
    if p_errno not in ("", "0"):
        # connection/auth レベル: 型付き例外で原因を切り分ける (raise)。
        check_response(payload)

    result_code = str(payload.get("sResultCode", ""))
    if result_code not in ("", "0"):
        return OrderAck(
            rejected=True,
            reject_code=result_code,
            reject_text=str(payload.get("sResultText", "")),
        )

    return OrderAck(
        rejected=False,
        order_number=str(payload.get("sOrderNumber", "")),
        eigyou_day=str(payload.get("sEigyouDay", "")),
        order_date=str(payload.get("sOrderDate", "")),
    )


# ---------------------------------------------------------------------------
# EC (注文約定通知) フレーム → 正規化済み約定レポート
# ---------------------------------------------------------------------------
#
# 情報コードのキー名・通知種別は e-station 参照実装 (architecture.md §6 /
# python/engine/exchanges/tachibana_event.py、`C:\Users\sasai\Documents\e-station`)
# で確定済み (2026-05-21):
#   p_NO  → venue_order_id (= sOrderNumber 相当)
#   p_EDA → trade_id (約定枝番。重複検知に使う。立花内部 p_eda_no)
#   p_NT  → notification_type (通知種別: 1=受付 / 2=約定 / 3=取消 / 4=失効)
#   p_DH  → last_price (約定単価。取消/失効時は欠落)
#   p_DSU → last_qty (約定数量。この約定分。取消/失効時は欠落)
#   p_ZSU → leaves_qty (残数量。0=全約定 → FILLED、>0 → PARTIALLY_FILLED)
#   p_OD  → 約定日時 (JST YYYYMMDDHHMMSS → UTC ms)
# EC は side / issue_code を持たない (注文セッション側で join)。本実装では adapter が
# venue_order_id→client_order_id を解決し、累計約定数量 = 発注数量 - leaves_qty で導出。
# ⚠️ 残る未確定は「口座レベル EC を購読する EVENT URL の構成」のみ (§5.1 layer-3)。
# ---------------------------------------------------------------------------

_EC_ORDER_NUMBER = "p_NO"  # 注文番号 (= venue_order_id)
_EC_TRADE_ID = "p_EDA"  # 約定枝番 (重複検知キー)
_EC_NOTIFY_TYPE = "p_NT"  # 通知種別
_EC_LAST_PRICE = "p_DH"  # 約定単価
_EC_LAST_QTY = "p_DSU"  # 約定数量 (この約定分)
_EC_LEAVES_QTY = "p_ZSU"  # 残数量
_EC_EXEC_DATETIME = "p_OD"  # 約定日時 (JST YYYYMMDDHHMMSS)

# p_NT 通知種別 (e-station tachibana_event.py:38-41)
_NT_RECEIVED = "1"  # 受付
_NT_FILLED = "2"  # 約定
_NT_CANCELED = "3"  # 取消
_NT_EXPIRED = "4"  # 失効


@dataclass(frozen=True)
class ExecutionReport:
    """EC フレームを正規化した約定/状態レポート (transport 非依存)。

    ``venue_order_id`` を adapter が client_order_id に解決して proto OrderEvent へ
    橋渡しする。本モジュールは client_order_id / 発注時数量を知らないため、累計約定
    数量 (cumulative filled) は adapter 側で ``発注数量 - leaves_qty`` から導出する。
    """

    venue_order_id: str
    trade_id: str
    notification_type: str
    last_price: float | None
    last_qty: float | None
    leaves_qty: float | None
    ts_event_ms: int


def parse_float(value: object) -> float:
    """Tachibana の数値文字列を float に。空/"*"/None は 0.0。"""
    try:
        if value in (None, "", "*"):
            return 0.0
        return float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return 0.0


def _parse_price_or_none(value: object) -> float | None:
    """約定単価。空/"*"/None/"0" は None (約定でない通知 = 価格なし)。"""
    if value in (None, "", "*", "0"):
        return None
    try:
        return float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def _parse_qty_or_none(value: object) -> float | None:
    """数量。空/"*"/None は None。ただし "0" は有効値 (leaves_qty=0 は全約定)。"""
    if value in (None, "", "*"):
        return None
    try:
        return float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return None


def _parse_exec_datetime_ms(p_od: str) -> int:
    """p_OD (約定日時 JST YYYYMMDDHHMMSS) を UTC ミリ秒に変換する。失敗時 0。"""
    if not p_od or len(p_od) < 14:
        return 0
    try:
        dt = datetime.strptime(p_od[:14], "%Y%m%d%H%M%S").replace(tzinfo=_JST)
        return int(dt.timestamp() * 1000)
    except ValueError:
        log.warning("tachibana EC: p_OD parse error %r", p_od)
        return 0


def ec_status(notification_type: str, leaves_qty: float | None) -> str:
    """通知種別 (+ 残数量) を Nautilus OrderStatus 名に写像する。

    約定 (2) は残数量 > 0 で PARTIALLY_FILLED、0/不明で FILLED。未知種別は ACCEPTED。
    """
    if notification_type == _NT_CANCELED:
        return "CANCELED"
    if notification_type == _NT_EXPIRED:
        return "EXPIRED"
    if notification_type == _NT_FILLED:
        if leaves_qty is not None and leaves_qty > 0:
            return "PARTIALLY_FILLED"
        return "FILLED"
    return "ACCEPTED"  # _NT_RECEIVED または未知種別


def parse_ec_frame(fields: dict[str, str]) -> ExecutionReport | None:
    """EC (注文約定通知) フレームの (key→value) dict を正規化する。

    注文番号 (p_NO) を持たないフレームは ``None`` を返す。
    """
    venue_order_id = fields.get(_EC_ORDER_NUMBER, "")
    if not venue_order_id:
        return None
    return ExecutionReport(
        venue_order_id=venue_order_id,
        trade_id=fields.get(_EC_TRADE_ID, ""),
        notification_type=fields.get(_EC_NOTIFY_TYPE, ""),
        last_price=_parse_price_or_none(fields.get(_EC_LAST_PRICE)),
        last_qty=_parse_qty_or_none(fields.get(_EC_LAST_QTY)),
        leaves_qty=_parse_qty_or_none(fields.get(_EC_LEAVES_QTY)),
        ts_event_ms=_parse_exec_datetime_ms(fields.get(_EC_EXEC_DATETIME, "")),
    )
