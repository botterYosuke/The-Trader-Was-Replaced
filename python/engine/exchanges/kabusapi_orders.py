"""kabuステーション 発注系 (sendorder / cancelorder / orders) のペイロード組み立て
& 応答パース (Phase 9 Step 6)。

純粋関数のみ (I/O なし)。HTTP 送信・約定 polling・OrderEvent push は
``KabuStationAdapter`` の責務であり本モジュールには持ち込まない (kabu skill: I/O は
adapter / kabusapi_auth に集約しつつ、組み立て・パースロジックは純粋関数でテスト可能に
する方針。tachibana_orders.py と同型)。

仕様根拠 (kabu skill R3/R7/R9 + OpenAPI ``RequestSendOrder`` / ``OrderSuccess`` /
``OrdersSuccess`` / ``RequestCancelOrder``):

- **Password フィールドは存在しない** (R3: 認証は X-API-KEY ヘッダのみ)。Tachibana の
  第二暗証番号にあたるものは kabu には無く、SecretVault / SecretRequired は発動しない。
- 現物 MVP (CashMargin=1 / SecurityType=1):
  - 買 (Side="2"): DelivType=2 (お預り金) / FundType="AA" (信用代用) — 公式サンプル準拠。
  - 売 (Side="1"): DelivType=0 (指定なし) / FundType="  " (半角スペース2つ) — OpenAPI 必須。
- ``FrontOrderType`` は order_type + time_in_force から導出 (成行/指値 × 当日/寄付/引け)。
- ``/sendorder`` 応答 (OrderSuccess): ``{"Result": 0, "OrderId": "..."}``。**``Result`` は
  発注エラーコードで、HTTP body の ``Code`` (4001xxx/4002xxx リクエストチェック) とは別系統**
  (§2.2 / R7)。``Code`` は ``kabusapi_auth.check_response`` が、``Result`` は本モジュールが判定する。
- ``/orders`` 応答 (OrdersSuccess[]): ``ID`` / ``State`` (5=終了) / ``OrderQty`` / ``CumQty``
  (累計約定数量) / ``Price`` / ``Details[]`` (RecType で取消/失効/約定を区別)。
"""
from __future__ import annotations

import logging
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone

log = logging.getLogger(__name__)

# Side (売買区分): kabu は 1=売 / 2=買 (文字列)。Tachibana の sBaibaiKubun とは値が違う。
_SIDE_TO_KABU: dict[str, str] = {"BUY": "2", "SELL": "1"}

# 現物 (CashMargin=1) 固定値。Phase 9 は現物のみ (信用/先物/OP は非対象)。
_SECURITY_TYPE_STOCK = 1
_CASH_MARGIN_CASH = 1

# 口座種別 (AccountType): MVP 既定は特定 (4)。kabu は login 応答に口座種別を載せない
# ため (Tachibana の sZyoutoekiKazeiC のような流用元が無い)、現物 MVP 定数とする。
# 一般 (2) / 法人 (12) 運用が必要になったら venue_params 経由で上書きする
# (Phase 9 Step 4/5 handoff「venue 固有発注パラメータ」)。adapter からも参照するので
# public 名 (アンダースコア無し)。
DEFAULT_ACCOUNT_TYPE = 4

# DelivType / FundType は買・売で非対称 (OpenAPI RequestSendOrder の必須条件)。
_DELIV_TYPE_BUY = 2  # お預り金
_DELIV_TYPE_SELL = 0  # 現物売は 0 (指定なし)
_FUND_TYPE_BUY = "AA"  # 信用代用 (公式サンプル kabusapi_sendorder_cash_buy.py)
_FUND_TYPE_SELL = "  "  # 半角スペース2つ (現物売の必須値)

# 約定日時 (orders Details の TransactTime) は JST。
_JST = timezone(timedelta(hours=9))


@dataclass(frozen=True)
class SendOrderAck:
    """``/sendorder`` 応答の正規化結果。

    ``rejected`` が True のときは発注エラー (Result != 0)。``order_id`` は採番されない。
    False のときは ``order_id`` (受付注文番号) が有効。
    """

    rejected: bool
    order_id: str = ""
    reject_code: str = ""
    reject_text: str = ""


@dataclass(frozen=True)
class OrderStatusReport:
    """``/orders`` の 1 注文を正規化した状態レポート (transport 非依存)。

    ``KabuStationAdapter`` がこれを venue_order_id→client_order_id 解決のうえ
    proto OrderEvent に橋渡しする。
    """

    order_id: str  # venue 採番の OrderId
    status: str  # Nautilus OrderStatus 名
    filled_qty: float  # 累計約定数量 (CumQty)
    avg_price: float  # 約定平均価格 (Details の約定明細から、無ければ Price)
    ts_ms: int  # 直近処理時刻 (UTC ms、取得失敗時 0)
    terminal: bool  # State==5 (これ以上 push 不要)


def side_to_kabu(side: str) -> str:
    """BUY/SELL → kabu Side ("2"/"1")。未知 side は ValueError。"""
    try:
        return _SIDE_TO_KABU[side.upper()]
    except KeyError as exc:
        raise ValueError(f"unknown side: {side!r}") from exc


def front_order_type(order_type: str, time_in_force: str) -> int:
    """order_type + time_in_force を kabu FrontOrderType (執行条件) に写像する。

    OrderPanel が送る time_in_force は DAY / OPENING / CLOSING の 3 値
    (src/ui/order_panel.rs)。MVP の前場/後場の解釈:
    - OPENING (寄付) → 前場寄り (寄成 13 / 寄指 21)。
    - CLOSING (引け) → 後場引け = 大引け (引成 16 / 引指 24)。
    - DAY / その他 → ザラバ成行 10 / 指値 20。
    """
    is_market = order_type.upper() == "MARKET"
    tif = time_in_force.upper()
    if tif == "OPENING":
        return 13 if is_market else 21
    if tif == "CLOSING":
        return 16 if is_market else 24
    return 10 if is_market else 20


def build_send_order_payload(
    *,
    symbol: str,
    exchange: int,
    side: str,
    qty: float,
    price: float | None,
    order_type: str,
    time_in_force: str,
    account_type: int = DEFAULT_ACCOUNT_TYPE,
) -> dict[str, object]:
    """現物 (CashMargin=1) の ``RequestSendOrder`` body を組み立てる。

    MARKET は ``Price=0`` (R9: 成行は 0)、LIMIT は指定価格。``Password`` フィールドは
    付与しない (R3)。買・売で DelivType / FundType を切り替える (OpenAPI 必須条件)。
    """
    side_n = side.upper()
    kabu_side = side_to_kabu(side_n)
    order_type_n = order_type.upper()
    if order_type_n == "MARKET":
        order_price: float = 0
    elif order_type_n == "LIMIT":
        if price is None:
            raise ValueError("LIMIT order requires a price")
        order_price = price
    else:
        raise ValueError(f"unknown order_type: {order_type!r}")

    is_buy = side_n == "BUY"
    return {
        "Symbol": symbol,
        "Exchange": exchange,
        "SecurityType": _SECURITY_TYPE_STOCK,
        "Side": kabu_side,
        "CashMargin": _CASH_MARGIN_CASH,
        "DelivType": _DELIV_TYPE_BUY if is_buy else _DELIV_TYPE_SELL,
        "FundType": _FUND_TYPE_BUY if is_buy else _FUND_TYPE_SELL,
        "AccountType": account_type,
        "Qty": int(qty),
        "FrontOrderType": front_order_type(order_type_n, time_in_force),
        "Price": order_price,
        "ExpireDay": 0,  # 0 = kabuステーションの「本日」(直近注文可能日)。期日指定は非対象。
    }


def build_cancel_order_payload(*, order_id: str) -> dict[str, str]:
    """``RequestCancelOrder`` body を組み立てる。``OrderID`` のみ・Password 不要 (R3)。"""
    if not order_id:
        raise ValueError("order_id is required for cancelorder")
    return {"OrderID": order_id}


def parse_send_order_response(payload: dict) -> SendOrderAck:
    """``/sendorder`` 応答を正規化する (HTTP/Code 判定後の二段目 = Result 判定)。

    呼び出し側 (adapter) が先に ``check_response`` で HTTP status と body ``Code``
    (リクエストチェックエラー 4001xxx/4002xxx) を判定し、本関数は ``Result`` フィールド
    (発注エラー、§2.2) のみを見る。``Result==0`` で正常 (OrderId 有効)、それ以外は
    発注リジェクト (SendOrderAck(rejected=True))。例外にはしない (REJECTED 注文として
    UI に反映するため。発注エラー Result=-1 等の致命例外は adapter が別途扱う)。
    """
    result = payload.get("Result", 0)
    try:
        result_code = int(result)
    except (TypeError, ValueError):
        result_code = -1
    if result_code != 0:
        return SendOrderAck(
            rejected=True,
            reject_code=str(result_code),
            reject_text=str(payload.get("Message", "")),
        )
    return SendOrderAck(rejected=False, order_id=str(payload.get("OrderId", "")))


# /orders State (注文全体): 5=終了 (発注エラー・取消済・全約定・失効・期限切れ)。
_ORDER_STATE_TERMINAL = 5

# Details RecType (明細種別): 3=期限切れ / 6=取消 / 7=失効 / 8=約定。
_RECTYPE_EXPIRED = 3
_RECTYPE_CANCELED = 6
_RECTYPE_VOIDED = 7
_RECTYPE_EXECUTION = 8

# Details State (明細状態) enum (OpenAPI OrdersSuccess.Details.State):
# 1=待機 / 2=処理中 / 3=処理済 (取消済・全約定・期限切れを含む唯一の完了状態) /
# 4=エラー / 5=削除済み。終端理由・約定平均の判定では「確定 = 処理済 (3)」のみを採用する。
# State==1 (待機) / ==2 (処理中) / ==4 (エラー) / ==5 (削除済み) は完了していないため、
# 取消/失効/約定の根拠として扱うと終端理由を誤分類する (review fix #1/#2)。
_DETAIL_STATE_DONE = 3


def parse_float(value: object) -> float:
    """kabu の数値を float に。None/空/非数は 0.0。"""
    if value is None or value == "":
        return 0.0
    try:
        return float(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return 0.0


def _avg_fill_price(details: list, fallback: float) -> float:
    """Details の約定明細 (RecType==8 かつ確定 State==3) から数量加重平均価格を求める。

    確定前 (State!=3 = 待機/処理中/エラー/削除済み) の約定行は終端理由判定 (#1) と同様に
    無視する。算入すると未確定の仮値が加重平均を汚染する (review fix #2)。確定約定が
    無ければ fallback (注文 Price) を返す。
    """
    total_qty = 0.0
    total_notional = 0.0
    for d in details:
        if not isinstance(d, dict) or d.get("RecType") != _RECTYPE_EXECUTION:
            continue
        if d.get("State") != _DETAIL_STATE_DONE:
            continue
        qty = parse_float(d.get("Qty"))
        px = parse_float(d.get("Price"))
        if qty > 0 and px > 0:
            total_qty += qty
            total_notional += qty * px
    if total_qty > 0:
        return total_notional / total_qty
    return fallback


def _confirmed_rectypes(details: list) -> set:
    """確定明細 (State==3 = 処理済) の RecType 集合。終端理由の判定に使う。

    処理済 (3) が完了を表す唯一の状態。待機 (1) / 処理中 (2) / エラー (4) / 削除済み (5) は
    完了していないため、取消/失効の根拠として数えると終端理由を誤分類する (review fix #1)。
    """
    return {
        d.get("RecType")
        for d in details
        if isinstance(d, dict) and d.get("State") == _DETAIL_STATE_DONE
    }


def _terminal_zero_fill_status(details: list) -> str:
    """約定ゼロで終了した注文の終端ステータスを Details から区別する。

    取消 (RecType=6) → CANCELED / 期限切れ (3) ・失効 (7) → EXPIRED / その他 → REJECTED。
    確定明細 (State!=2) のみを根拠にする。
    """
    rectypes = _confirmed_rectypes(details)
    if _RECTYPE_CANCELED in rectypes:
        return "CANCELED"
    if _RECTYPE_EXPIRED in rectypes or _RECTYPE_VOIDED in rectypes:
        return "EXPIRED"
    return "REJECTED"


def _terminal_remainder_status(details: list) -> str:
    """部分約定したまま終了した注文の「残数量」の終端理由を Details から区別する。

    残りが期限切れ (3) ・失効 (7) なら EXPIRED、それ以外 (取消 6 を含む) は CANCELED。
    取消明細と失効明細が混在する場合は取消優先 (CANCELED)。約定ゼロ時の REJECTED 既定
    とは異なり、部分約定済みの注文がブローカー REJECT で終わることはないため CANCELED を
    既定にする。
    """
    rectypes = _confirmed_rectypes(details)
    if (
        _RECTYPE_EXPIRED in rectypes or _RECTYPE_VOIDED in rectypes
    ) and _RECTYPE_CANCELED not in rectypes:
        return "EXPIRED"
    return "CANCELED"


def order_status(
    *, state: int, order_qty: float, cum_qty: float, details: list
) -> str:
    """注文全体の State + 約定量 + Details から Nautilus OrderStatus 名を導出する。

    - State==5 (終了): 全約定→FILLED / 一部約定して終了→CANCELED (残り取消扱い) /
      約定ゼロ→Details で CANCELED/EXPIRED/REJECTED を区別。
    - State 1-4 (非終端): 約定>0→PARTIALLY_FILLED / 約定ゼロ→ACCEPTED。
    """
    if state == _ORDER_STATE_TERMINAL:
        if order_qty > 0 and cum_qty >= order_qty:
            return "FILLED"
        if cum_qty > 0:
            # 部分約定したまま終了 = 残数量は取消 or 失効。Details で区別する
            # (取消→CANCELED / 期限切れ・失効→EXPIRED)。
            return _terminal_remainder_status(details)
        return _terminal_zero_fill_status(details)
    if cum_qty > 0:
        return "PARTIALLY_FILLED"
    return "ACCEPTED"


def _parse_transact_time_ms(details: list) -> int:
    """Details の最新 TransactTime (JST "yyyyMMddHHmmss" 等) を UTC ms に。失敗時 0。"""
    times = [
        str(d.get("TransactTime", ""))
        for d in details
        if isinstance(d, dict) and d.get("TransactTime")
    ]
    if not times:
        return 0
    raw = max(times)  # 文字列としても時系列順 (yyyyMMdd...) を保つ
    digits = "".join(ch for ch in raw if ch.isdigit())
    if len(digits) < 14:
        return 0
    try:
        dt = datetime.strptime(digits[:14], "%Y%m%d%H%M%S").replace(tzinfo=_JST)
        return int(dt.timestamp() * 1000)
    except ValueError:
        return 0


def parse_order_status(order: dict) -> OrderStatusReport | None:
    """``/orders`` の 1 要素 (OrdersSuccess) を ``OrderStatusReport`` に正規化する。

    ``ID`` を持たない要素は ``None``。
    """
    order_id = str(order.get("ID", ""))
    if not order_id:
        return None
    state = int(parse_float(order.get("State", order.get("OrderState", 0))))
    if state not in (1, 2, 3, 4, 5):
        # kabu /orders の State は OpenAPI 上 1-5 のみ。範囲外/欠損は想定外データなので
        # 0→ACCEPTED と誤魔化さず (誤 ACCEPTED は UI に偽の生存注文を出す) 当該行を
        # スキップする。値が一時的に異常でも次回 poll で 1-5 に落ち着いて parse される。
        log.debug(
            "kabu /orders row %r has out-of-range State=%r; skipping", order_id, state
        )
        return None
    order_qty = parse_float(order.get("OrderQty"))
    cum_qty = parse_float(order.get("CumQty"))
    details = order.get("Details") or []
    if not isinstance(details, list):
        details = []
    status = order_status(
        state=state, order_qty=order_qty, cum_qty=cum_qty, details=details
    )
    avg_price = _avg_fill_price(details, parse_float(order.get("Price")))
    return OrderStatusReport(
        order_id=order_id,
        status=status,
        filled_qty=cum_qty,
        avg_price=avg_price,
        ts_ms=_parse_transact_time_ms(details),
        terminal=state == _ORDER_STATE_TERMINAL,
    )
