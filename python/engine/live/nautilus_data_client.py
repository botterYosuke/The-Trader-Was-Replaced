"""engine.live.nautilus_data_client — live venue tick を Nautilus に注入する
`LiveMarketDataClient` (Phase 10 §2.3 / Step 8 本丸)。

戦略が `self.subscribe_bars(<...-INTERNAL>)` を呼ぶと、`LiveDataEngine` は INTERNAL
集約のために内部 `TimeBarAggregator` を作り、その venue 宛に `SubscribeTradeTicks` を
発行する。本 client はその subscribe を受理（記録するだけ）し、live セッションから来た
約定を `feed_trades_update()` → `_handle_data(TradeTick)` で engine に流す。engine は
`TradeTick` を trades topic に publish し、aggregator が確定 `Bar` を組んで戦略の
`on_bar` に届ける——これで Replay（catalog の EXTERNAL `Bar`）と Live（aggregation 由来の
INTERNAL `Bar`）が同じ `Bar` 型・同じ `BarSpecification` に揃う（ADR-B）。

所有権（§1.1）: venue への接続は Phase 9 live session が既に張っている。本 client は
新たな login / WebSocket を作らず、`engine_controller` が `LiveRunner` の tick tap に
登録した listener から `feed_trades_update()` を受け取るだけ（exec client と同じ共有方針）。

注意:
- `_handle_data` は `msgbus.send("DataEngine.process", tick)` するだけで、接続状態に
  依存しない（`data/client.pyx:_handle_data`）。kernel/engine が start 済みであればよい。
- venue 別入力（M2）: Tachibana は EC 約定がそのまま `TradesUpdate`。kabu は約定 tick が
  無いため板 `CurrentPrice` / polling から `TradesUpdate` 相当を構成する必要がある
  （精度限界は §8 Open Risk 5。本 client は `TradesUpdate` を受ければ venue 非依存に動く）。
"""

from __future__ import annotations

import logging
from typing import Any

from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId, InstrumentId, Venue

from engine.live.bar_supply import trades_update_to_trade_tick

log = logging.getLogger(__name__)


class NautilusVenueDataClient(LiveMarketDataClient):
    """live venue の約定 tick を Nautilus aggregation に橋渡しする data client。

    発注主体（StrategyId）は exec 経路の責務。data 経路は venue 単位で単一でよい。
    """

    def __init__(
        self,
        *,
        loop,
        venue: Venue,
        msgbus,
        cache,
        clock,
        instrument_provider,
        config=None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(venue.value),
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
        # 同一 ns の複数約定でも trade_id を一意にするための単調カウンタ。
        self._seq = 0
        # per-tick の `InstrumentId.from_str` を避けるための parse メモ（venue id → InstrumentId）。
        self._iid_cache: dict[str, InstrumentId] = {}

    # ── connection（共有 session なので張り直さない）─────────────────────────────

    async def _connect(self) -> None:
        pass

    async def _disconnect(self) -> None:
        pass

    # ── subscriptions ────────────────────────────────────────────────────────────
    # base クラスの sync ラッパが購読集合を記録するので、coroutine 側は no-op でよい。
    # INTERNAL bar は engine が aggregator を作って trades を要求するため、ここで
    # 受理だけする（実 venue 購読は LiveRunner / adapter が別途行う、共有 session）。

    async def _subscribe_trade_ticks(self, command) -> None:
        pass

    async def _unsubscribe_trade_ticks(self, command) -> None:
        pass

    async def _subscribe_quote_ticks(self, command) -> None:
        pass

    async def _unsubscribe_quote_ticks(self, command) -> None:
        pass

    # ── tick 注入 ─────────────────────────────────────────────────────────────────

    def feed_trades_update(self, trade: Any) -> None:
        """venue の `TradesUpdate` を `TradeTick` 化して engine に注入する。

        cache に instrument が無い（attach で登録前 / 別銘柄）場合は黙ってスキップする。
        live loop thread から同期呼び出しされる前提（`_handle_data` は msgbus.send のみで
        blocking しない）。
        """
        raw = str(trade.instrument_id)
        iid = self._iid_cache.get(raw)
        if iid is None:
            iid = InstrumentId.from_str(raw)
            self._iid_cache[raw] = iid
        instrument = self._cache.instrument(iid)
        if instrument is None:
            return
        self._seq += 1
        tick = trades_update_to_trade_tick(trade, instrument, self._seq)
        self._handle_data(tick)
