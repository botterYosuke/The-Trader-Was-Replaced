"""_proto_compat.py — proto3 optional / oneof を模倣する純 Python 互換層.

純 Python で実装されており、外部ライブラリに依存しない。
`from . import _proto_compat as engine_pb2` として利用する。
"""
from __future__ import annotations

from typing import Optional


# ---------------------------------------------------------------------------
# enum 定数
# ---------------------------------------------------------------------------

# EngineState
IDLE = 0
LOADED = 1
RUNNING = 2
PAUSED = 3
STOPPING = 4

# EngineKind
NAUTILUS = 0
SIMULATED = 1

# ReplayGranularity
TICK = 0
SECOND = 1
MINUTE = 2
DAILY = 3


# ---------------------------------------------------------------------------
# Mixin: proto3 optional フィールドの HasField サポート
# ---------------------------------------------------------------------------

class _HasFieldMixin:
    def __init__(self, **kwargs):
        self._set_fields: set = set()
        for k, v in kwargs.items():
            object.__setattr__(self, k, v)
            if v is not None:
                self._set_fields.add(k)

    def __setattr__(self, name: str, value):
        object.__setattr__(self, name, value)
        if name.startswith("_"):
            return
        if value is not None:
            self._set_fields.add(name)
        else:
            self._set_fields.discard(name)

    def HasField(self, name: str) -> bool:
        return name in self._set_fields


# ---------------------------------------------------------------------------
# Mixin: BackendEvent oneof payload の WhichOneof サポート
# ---------------------------------------------------------------------------

class _OneofMixin(_HasFieldMixin):
    _PAYLOAD_FIELDS = frozenset({
        "secret_required",
        "order_event",
        "account_event",
        "venue_logout_detected",
        "live_strategy_event",
        "safety_rail_violation",
        "strategy_log_message",
        "live_strategy_telemetry",
        "backend_error",
    })

    def WhichOneof(self, oneof_name: str) -> Optional[str]:
        if oneof_name != "payload":
            raise ValueError(f"Unknown oneof: {oneof_name!r}")
        for field in self._PAYLOAD_FIELDS:
            if field in self._set_fields:
                return field
        return None


# ---------------------------------------------------------------------------
# OrderEvent（price / strategy_id が proto3 optional）
# ---------------------------------------------------------------------------

class OrderEvent(_HasFieldMixin):
    def __init__(
        self,
        order_id: str = "",
        venue_order_id: str = "",
        client_order_id: str = "",
        status: str = "",
        filled_qty: float = 0.0,
        avg_price: float = 0.0,
        ts_ms: int = 0,
        strategy_id: Optional[str] = None,
        symbol: str = "",
        side: str = "",
        qty: float = 0.0,
        price: Optional[float] = None,
    ):
        self._set_fields: set = set()
        self.order_id = order_id
        self.venue_order_id = venue_order_id
        self.client_order_id = client_order_id
        self.status = status
        self.filled_qty = filled_qty
        self.avg_price = avg_price
        self.ts_ms = ts_ms
        self.symbol = symbol
        self.side = side
        self.qty = qty
        if strategy_id is not None:
            self.strategy_id = strategy_id
            self._set_fields.add("strategy_id")
        else:
            self.strategy_id = ""
        if price is not None:
            self.price = price
            self._set_fields.add("price")
        else:
            self.price = 0.0

    def __setattr__(self, name: str, value):
        object.__setattr__(self, name, value)
        if name.startswith("_"):
            return
        if name in ("price", "strategy_id"):
            if value is None:
                self._set_fields.discard(name)
            else:
                self._set_fields.add(name)
        elif value is not None:
            self._set_fields.add(name)


# ---------------------------------------------------------------------------
# AccountPosition
# ---------------------------------------------------------------------------

class AccountPosition:
    def __init__(
        self,
        symbol: str = "",
        qty: int = 0,
        avg_price: float = 0.0,
        unrealized_pnl: float = 0.0,
    ):
        self.symbol = symbol
        self.qty = qty
        self.avg_price = avg_price
        self.unrealized_pnl = unrealized_pnl


# ---------------------------------------------------------------------------
# AccountEvent
# ---------------------------------------------------------------------------

class AccountEvent:
    def __init__(
        self,
        cash: float = 0.0,
        buying_power: float = 0.0,
        positions: Optional[list] = None,
        ts_ms: int = 0,
    ):
        self.cash = cash
        self.buying_power = buying_power
        self.positions = positions if positions is not None else []
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# SecretRequired
# ---------------------------------------------------------------------------

class SecretRequired:
    def __init__(self, request_id: str = "", venue: str = "", kind: str = "", purpose: str = ""):
        self.request_id = request_id
        self.venue = venue
        self.kind = kind
        self.purpose = purpose


# ---------------------------------------------------------------------------
# VenueLogoutDetected
# ---------------------------------------------------------------------------

class VenueLogoutDetected:
    def __init__(self, venue: str = ""):
        self.venue = venue


# ---------------------------------------------------------------------------
# LiveStrategyEvent
# ---------------------------------------------------------------------------

class LiveStrategyEvent:
    def __init__(self, run_id: str = "", strategy_id: str = "", status: str = "", ts_ms: int = 0):
        self.run_id = run_id
        self.strategy_id = strategy_id
        self.status = status
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# SafetyRailViolation
# ---------------------------------------------------------------------------

class SafetyRailViolation:
    def __init__(self, run_id: str = "", kind: str = "", detail: str = "", ts_ms: int = 0):
        self.run_id = run_id
        self.kind = kind
        self.detail = detail
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# BackendError
# ---------------------------------------------------------------------------

class BackendError:
    def __init__(self, source: str = "", detail: str = "", ts_ms: int = 0):
        self.source = source
        self.detail = detail
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# StrategyLogMessage
# ---------------------------------------------------------------------------

class StrategyLogMessage:
    def __init__(self, run_id: str = "", level: str = "", message: str = "", ts_ms: int = 0):
        self.run_id = run_id
        self.level = level
        self.message = message
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# LiveStrategyTelemetry
# ---------------------------------------------------------------------------

class LiveStrategyTelemetry:
    def __init__(
        self,
        run_id: str = "",
        strategy_id: str = "",
        realized_pnl: float = 0.0,
        unrealized_pnl: float = 0.0,
        order_count: int = 0,
        fill_count: int = 0,
        ts_ms: int = 0,
    ):
        self.run_id = run_id
        self.strategy_id = strategy_id
        self.realized_pnl = realized_pnl
        self.unrealized_pnl = unrealized_pnl
        self.order_count = order_count
        self.fill_count = fill_count
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# BackendEvent（oneof payload）
# ---------------------------------------------------------------------------

class BackendEvent(_OneofMixin):
    def __init__(
        self,
        secret_required=None,
        order_event=None,
        account_event=None,
        venue_logout_detected=None,
        live_strategy_event=None,
        safety_rail_violation=None,
        strategy_log_message=None,
        live_strategy_telemetry=None,
        backend_error=None,
    ):
        self._set_fields: set = set()
        for field, value in (
            ("secret_required", secret_required),
            ("order_event", order_event),
            ("account_event", account_event),
            ("venue_logout_detected", venue_logout_detected),
            ("live_strategy_event", live_strategy_event),
            ("safety_rail_violation", safety_rail_violation),
            ("strategy_log_message", strategy_log_message),
            ("live_strategy_telemetry", live_strategy_telemetry),
            ("backend_error", backend_error),
        ):
            object.__setattr__(self, field, value)
            if value is not None:
                self._set_fields.add(field)


# ---------------------------------------------------------------------------
# PlaceOrderRes / CancelOrderRes / ModifyOrderRes / GetOrderStatusRes
# （HasField("order_event") が必要）
# ---------------------------------------------------------------------------

class PlaceOrderRes(_HasFieldMixin):
    def __init__(self, success: bool = False, error_code: str = "", order_event: Optional[OrderEvent] = None):
        self._set_fields: set = set()
        self.success = success
        self.error_code = error_code
        if order_event is not None:
            self.order_event = order_event
            self._set_fields.add("order_event")
        else:
            self.order_event = OrderEvent()


class CancelOrderRes(_HasFieldMixin):
    def __init__(self, success: bool = False, error_code: str = "", order_event: Optional[OrderEvent] = None):
        self._set_fields: set = set()
        self.success = success
        self.error_code = error_code
        if order_event is not None:
            self.order_event = order_event
            self._set_fields.add("order_event")
        else:
            self.order_event = OrderEvent()


class ModifyOrderRes(_HasFieldMixin):
    def __init__(self, success: bool = False, error_code: str = "", order_event: Optional[OrderEvent] = None):
        self._set_fields: set = set()
        self.success = success
        self.error_code = error_code
        if order_event is not None:
            self.order_event = order_event
            self._set_fields.add("order_event")
        else:
            self.order_event = OrderEvent()


class GetOrderStatusRes(_HasFieldMixin):
    def __init__(self, success: bool = False, error_code: str = "", order_event: Optional[OrderEvent] = None):
        self._set_fields: set = set()
        self.success = success
        self.error_code = error_code
        if order_event is not None:
            self.order_event = order_event
            self._set_fields.add("order_event")
        else:
            self.order_event = OrderEvent()


# ---------------------------------------------------------------------------
# GetOrdersRes
# ---------------------------------------------------------------------------

class GetOrdersRes:
    def __init__(self, success: bool = False, error_code: str = "", orders: Optional[list] = None):
        self.success = success
        self.error_code = error_code
        self.orders = orders if orders is not None else []


# ---------------------------------------------------------------------------
# SafetyLimits
# ---------------------------------------------------------------------------

class SafetyLimits:
    def __init__(
        self,
        max_position_size_jpy: int = 0,
        max_order_value_jpy: int = 0,
        max_daily_loss_jpy: int = 0,
        max_orders_per_minute: int = 0,
        allowed_instruments: Optional[list] = None,
    ):
        self.max_position_size_jpy = max_position_size_jpy
        self.max_order_value_jpy = max_order_value_jpy
        self.max_daily_loss_jpy = max_daily_loss_jpy
        self.max_orders_per_minute = max_orders_per_minute
        self.allowed_instruments = allowed_instruments if allowed_instruments is not None else []


# ---------------------------------------------------------------------------
# LiveStrategyStatus
# ---------------------------------------------------------------------------

class LiveStrategyStatus:
    def __init__(
        self,
        run_id: str = "",
        strategy_id: str = "",
        nautilus_strategy_id: str = "",
        instrument_id: str = "",
        venue: str = "",
        status: str = "",
        ts_ms: int = 0,
    ):
        self.run_id = run_id
        self.strategy_id = strategy_id
        self.nautilus_strategy_id = nautilus_strategy_id
        self.instrument_id = instrument_id
        self.venue = venue
        self.status = status
        self.ts_ms = ts_ms


# ---------------------------------------------------------------------------
# シンプルなメッセージファクトリ（HasField 不要なメッセージ群）
# ---------------------------------------------------------------------------

def _simple(**fields):
    """repeated フィールドは [] デフォルトで、インスタンスごとにコピーを割り当てる."""
    cls_fields = fields

    def __init__(self, **kwargs):
        for k, v in cls_fields.items():
            if k in kwargs:
                object.__setattr__(self, k, kwargs[k])
            elif isinstance(v, list):
                object.__setattr__(self, k, list(v))
            elif isinstance(v, dict):
                object.__setattr__(self, k, dict(v))
            else:
                object.__setattr__(self, k, v)

    def HasField(self, _name: str) -> bool:  # noqa: N802
        return False

    return type("_Msg", (), {"__init__": __init__, "HasField": HasField})


# Request メッセージ
HealthCheckRequest          = _simple(service="")
GetStateRequest             = _simple(token="")
StartRequest                = _simple(token="")
StopRequest                 = _simple(token="")
LoadReplayDataRequest       = _simple(request_id="", instrument_ids=None, start_date="", end_date="", granularity=None, token="", catalog_path=None)
EngineStartConfig           = _simple(instrument_id="", instrument_ids=None, start_date=None, end_date=None, initial_cash=None, granularity=None, strategy_file=None, strategy_init_kwargs=None, max_qty=None, max_notional_jpy=None)
StartEngineRequest          = _simple(request_id="", engine=NAUTILUS, strategy_id="", config=None, token="")
StopEngineRequest           = _simple(request_id="", token="")
SetReplaySpeedRequest       = _simple(request_id="", multiplier=1, token="")
PauseReplayRequest          = _simple(request_id="", token="")
ResumeReplayRequest         = _simple(request_id="", token="")
StepReplayRequest           = _simple(request_id="", token="")
StopReplayRequest           = _simple(request_id="", token="")
ForceStopReplayRequest      = _simple(request_id="", token="")
ListInstrumentsRequest      = _simple(token="", source=None)
Instrument                  = _simple(id="", name="", market="")
ListAllListedSymbolsRequest = _simple(token="", end_date="")
GetPortfolioRequest         = _simple(token="")
PortfolioPosition           = _simple(symbol="", qty=0, avg_price=0.0, unrealized_pnl=0.0)
PortfolioOrder              = _simple(symbol="", side="", qty=0.0, price=0.0, status="", ts_ms=0)
VenueLoginRequest           = _simple(venue_id="", credentials_source="", environment_hint="", token="")
VenueLogoutRequest          = _simple(token="")
SubscribeRequest            = _simple(instrument_id="", channels=None, token="")
UnsubscribeRequest          = _simple(instrument_id="", token="")
SetExecutionModeRequest     = _simple(mode="", token="")
ForceAccountSnapshotRequest = _simple(token="")
ShutdownRequest             = _simple(token="", grace_seconds=0)
SubscribeBackendEventsReq   = _simple(token="")
SubmitSecretReq             = _simple(token="", request_id="", secret="")
PlaceOrderReq               = _simple(token="", venue="", instrument_id="", side="", qty=0.0, price=None, order_type="", time_in_force="", second_secret=None)
CancelOrderReq              = _simple(token="", venue="", order_id="", second_secret=None)
ModifyOrderReq              = _simple(token="", venue="", order_id="", new_price=None, new_qty=None, second_secret=None)
GetOrderStatusReq           = _simple(token="", venue="", order_id="")
GetOrdersReq                = _simple(token="", venue="")
RegisterLiveStrategyReq     = _simple(token="", request_id="", strategy_file="", expected_sha256="")
StartLiveStrategyReq        = _simple(token="", request_id="", strategy_id="", instrument_id="", venue="", params={}, safety_limits=None)
StopLiveStrategyReq         = _simple(token="", request_id="", run_id="")
PauseLiveStrategyReq        = _simple(token="", request_id="", run_id="")
ResumeLiveStrategyReq       = _simple(token="", request_id="", run_id="")
GetLiveStrategyStatusReq    = _simple(token="", request_id="", run_id="")
ListLiveStrategiesReq       = _simple(token="", request_id="")

# Response メッセージ
HealthCheckResponse         = _simple(status=0)
GetStateResponse            = _simple(json_data="")
StartResponse               = _simple(success=False)
StopResponse                = _simple(success=False)
ReplayControlResponse       = _simple(success=False, request_id="", current_state=IDLE, error_code="", error_message="")
StartEngineResponse         = _simple(success=False, request_id="", current_state=IDLE, error_code=None, error_message=None, run_id=None, summary_json=None)
ListInstrumentsResponse     = _simple(success=False, instrument_ids=[], error_message="", instruments=[])
ListAllListedSymbolsResponse = _simple(success=False, instrument_ids=[], error_message="", resolved_end_date="")
GetPortfolioResponse        = _simple(success=False, buying_power=0.0, cash=0.0, equity=0.0, positions=[], orders=[], error_message="")
VenueLoginResponse          = _simple(success=False, error_code="", venue_state="", instruments_loaded=0)
VenueControlResponse        = _simple(success=False, error_code="")
SubscribeResponse           = _simple(success=False, error_code="")
SetExecutionModeResponse    = _simple(success=False, error_code="", execution_mode="")
ForceAccountSnapshotResponse = _simple(success=False, error_code="")
ShutdownResponse            = _simple(accepted=False, error_code="")
SubmitSecretRes             = _simple(success=False, error_code="")
RegisterLiveStrategyRes     = _simple(success=False, request_id="", error_code="", strategy_id="", strategy_sha256="", display_name="", error_message="")
StartLiveStrategyRes        = _simple(success=False, request_id="", error_code="", run_id="", status=None, error_message="")
LiveStrategyControlRes      = _simple(success=False, request_id="", error_code="", status=None)
GetLiveStrategyStatusRes    = _simple(success=False, request_id="", error_code="", status=None)
ListLiveStrategiesRes       = _simple(success=False, request_id="", error_code="", strategies=[])
