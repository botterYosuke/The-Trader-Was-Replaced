"""LiveVenueAdapter Protocol and related type skeletons.

Phase 8 §1.3 で定義した、venue 非依存の adapter インターフェース。
Tachibana / kabu 等の具体実装は後続 step で追加する。

LiveEvent は KlineUpdate / TradesUpdate / DepthUpdate の discriminated
union（kind discriminator）。
"""

from __future__ import annotations

from typing import (
    TYPE_CHECKING,
    Annotated,
    AsyncIterator,
    Literal,
    Protocol,
    Union,
    runtime_checkable,
)

from pydantic import BaseModel, Field, model_validator

if TYPE_CHECKING:
    from engine.live.order_types import AccountSnapshot, OrderResult

# --- 基本型エイリアス ---

InstrumentId = str
"""Nautilus InstrumentId 文字列形式（例: '7203.TSE'）。
Nautilus 型への変換は別 step（reducer 側）で行う。"""

Channel = Literal["price", "trades", "depth"]
"""購読チャネル種別。venue 横断で共通。"""

# --- credentials / instrument の骨格 ---

class VenueCredentials(BaseModel):
    """ログイン要求の入力。

    重要: 平文 password を含まない。Phase 8 §3.2 で定義した
    credentials_source ベース（prompt / session_cache / env）で
    resolve する。具体的な credential 値は adapter 内部で
    subprocess / env / cache から取得する。
    """

    credentials_source: Literal["prompt", "session_cache", "env", "prompt_result"]
    environment_hint: str | None = None  # "prod" / "demo" 等のヒント
    token: str | None = None  # kabu prompt_result 専用

    model_config = {"frozen": True}

    @model_validator(mode="after")
    def _validate_prompt_result_requires_token(self) -> "VenueCredentials":
        if self.credentials_source == "prompt_result" and not self.token:
            raise ValueError(
                "credentials_source='prompt_result' requires a non-empty token"
            )
        return self


class InstrumentRaw(BaseModel):
    """venue が返す instrument の生形式。

    Nautilus Instrument への正規化は別 step。最小フィールドのみ。
    """

    code: str  # 銘柄コード（例: "7203"）
    name: str  # 銘柄名
    market: str  # 市場コード（例: "TSE"）
    tick_size: float
    lot_size: int

    model_config = {"frozen": True}


# --- Market data event union ---


class KlineUpdate(BaseModel):
    """OHLCV bar update（Replay の KlineUpdate と同形式）。"""

    kind: Literal["kline"]
    instrument_id: InstrumentId
    ts_ns: int
    open: float
    high: float
    low: float
    close: float
    volume: float

    model_config = {"frozen": True}


class TradesUpdate(BaseModel):
    """単一約定 tick。"""

    kind: Literal["trades"]
    instrument_id: InstrumentId
    ts_ns: int
    price: float
    size: float
    aggressor_side: Literal["buy", "sell"]

    model_config = {"frozen": True}


class DepthLevel(BaseModel):
    """板の 1 段（price/size のみ）。"""

    price: float
    size: float

    model_config = {"frozen": True}


class DepthUpdate(BaseModel):
    """板更新（bids/asks 各 0-10 段、空も許容）。"""

    kind: Literal["depth"]
    instrument_id: InstrumentId
    ts_ns: int
    bids: Annotated[tuple[DepthLevel, ...], Field(max_length=10)]
    asks: Annotated[tuple[DepthLevel, ...], Field(max_length=10)]

    model_config = {"frozen": True}


LiveEvent = Annotated[
    Union[KlineUpdate, TradesUpdate, DepthUpdate],
    Field(discriminator="kind"),
]
"""price / trades / depth update の discriminated union（kind discriminator）。

reducer 側は `kind` フィールドで分岐する。pydantic v2 の
`TypeAdapter(LiveEvent).validate_python(...)` でも分岐可能。
"""


# --- Adapter Protocol ---

@runtime_checkable
class LiveVenueAdapter(Protocol):
    """venue 非依存の live adapter インターフェース（Phase 8 §1.3）。

    実装は asyncio タスクとして動き、events() から非同期に
    market data event を yield する。
    """

    venue_id: str  # "TACHIBANA" / "KABU"

    @property
    def is_logged_in(self) -> bool: ...

    async def login(self, creds: VenueCredentials) -> None: ...
    async def logout(self) -> None: ...
    async def fetch_instruments(self) -> list[InstrumentRaw]: ...
    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None: ...
    async def unsubscribe(self, instrument_id: InstrumentId) -> None: ...
    def events(self) -> AsyncIterator[LiveEvent]: ...


@runtime_checkable
class OrderingVenueAdapter(LiveVenueAdapter, Protocol):
    """発注可能な venue adapter（Phase 9）。LiveVenueAdapter に手動発注経路を足す。

    Phase 9 Step 2 の ManualOrderFacade はこの契約に依存する。MockVenueAdapter は
    既にこれを満たし、Tachibana / kabu の具体実装は Step 5/6 でこの契約を満たすこと
    （発注は本来 ExecutionClient の責務だが、Step 2 では adapter に薄く委譲する。
    真正 Nautilus ExecEngine wiring は Phase 10 / LiveAuto、ADR §7 参照）。
    submit_order / cancel_order は OrderResult を返す（engine.live.order_types）。
    """

    async def submit_order(
        self,
        *,
        venue: str,
        instrument_id: InstrumentId,
        side: str,
        qty: float,
        price: float | None,
        order_type: str,
        time_in_force: str,
        **extra: object,
    ) -> "OrderResult": ...

    async def cancel_order(
        self,
        *,
        venue: str,
        order_id: str,
        **extra: object,
    ) -> "OrderResult": ...

    async def modify_order(
        self,
        *,
        venue: str,
        order_id: str,
        new_price: float | None = None,
        new_qty: float | None = None,
        **extra: object,
    ) -> "OrderResult":
        """既存注文の訂正（価格 / 数量）。OrderResult を返す。

        venue 別の実体（Step 5/6 の adapter 実装の責務、Step 4 は mock のみ）:
        - **Tachibana**: `CLMKabuCorrectOrder`（venue 側 atomic な訂正 API）に直接マップ。
        - **kabu**: 訂正 API が無いため adapter 内部で「取消 → 新規発注」に変換する
          （atomicity は保証されない。UI に警告バナーを出すのは §3.11 / Step 6）。
        """
        ...

    async def fetch_account(self) -> "AccountSnapshot": ...

    async def fetch_working_orders(self) -> "list[OrderEventData]": ...
