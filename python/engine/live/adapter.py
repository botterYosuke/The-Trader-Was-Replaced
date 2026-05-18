"""LiveVenueAdapter Protocol and related type skeletons.

Phase 8 §1.3 で定義した、venue 非依存の adapter インターフェース。
Tachibana / kabu 等の具体実装は後続 step で追加する。

LiveEvent は初期スタブ（object alias）。KlineUpdate / TradesUpdate /
DepthUpdate への置き換えは Phase 8 後半（reducer 実装時）で行う。
"""

from __future__ import annotations

from typing import AsyncIterator, Literal, Protocol, runtime_checkable

from pydantic import BaseModel

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

    credentials_source: Literal["prompt", "session_cache", "env"]
    environment_hint: str | None = None  # "prod" / "demo" 等のヒント

    model_config = {"frozen": True}


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


# --- LiveEvent スタブ ---

LiveEvent = object
"""price / trades / depth update の union 型。

Phase 8 初期段階のスタブ。後続 step（reducer 実装時）で
KlineUpdate | TradesUpdate | DepthUpdate に置き換える。
"""


# --- Adapter Protocol ---

@runtime_checkable
class LiveVenueAdapter(Protocol):
    """venue 非依存の live adapter インターフェース（Phase 8 §1.3）。

    実装は asyncio タスクとして動き、events() から非同期に
    market data event を yield する。
    """

    venue_id: str  # "TACHIBANA" / "KABU"

    async def login(self, creds: VenueCredentials) -> None: ...
    async def logout(self) -> None: ...
    async def fetch_instruments(self) -> list[InstrumentRaw]: ...
    async def subscribe(
        self, instrument_id: InstrumentId, channels: set[Channel]
    ) -> None: ...
    async def unsubscribe(self, instrument_id: InstrumentId) -> None: ...
    def events(self) -> AsyncIterator[LiveEvent]: ...
