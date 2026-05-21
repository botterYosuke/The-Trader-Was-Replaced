"""engine.strategy_runtime.engine_runner — BacktestEngine streaming runner (Step 3A).

BlackSheep 戦略を 1 bar ずつ BacktestEngine に通す最小ランナー。

Public API:
    RunBufferLike   — write_fill / write_equity を持つ Protocol
    run(...)        — streaming loop のエントリポイント

Step 3A の意図的な省略:
    - pause / step / restore
    - DateChangeMarker / ReplayTimeUpdated
    - wallclock pacing
    - order_flow_06 warmup
    - real RunBuffer
    - blacksheep ingest
"""

from __future__ import annotations

import logging
import threading
from typing import Protocol, runtime_checkable

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig, LoggingConfig
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.enums import AccountType, OmsType
from nautilus_trader.model.identifiers import InstrumentId, Venue
from nautilus_trader.model.objects import Money

from engine.strategy_runtime.catalog_data_loader import (
    bar_type_for_instrument,
    instruments_from_scenario,
    merge_bars_by_ts,
    normalize_granularity,
)
from engine.strategy_runtime.instrument_factory import make_equity_instrument

log = logging.getLogger(__name__)

_BYPASS_LOG = LoggingConfig(bypass_logging=True)
_GRANULARITY_TO_BAR_PERIOD: dict[str, str] = {
    "Daily": "DAY",
    "Minute": "MINUTE",
}


# ---------------------------------------------------------------------------
# Protocol
# ---------------------------------------------------------------------------


@runtime_checkable
class RunBufferLike(Protocol):
    def write_fill(self, event: dict) -> None: ...  # noqa: E704
    def write_equity(self, event: dict) -> None: ...  # noqa: E704


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------


def run(
    *,
    strategy_cls,
    scenario: dict,
    bars_by_instrument: dict,
    run_buffer: RunBufferLike,
    strategy_init_kwargs: dict | None = None,
    run_event: threading.Event | None = None,
    bar_interval_sec: float = 0.0,
) -> None:
    """BacktestEngine を使って 1 bar ずつ streaming replay する (Step 3A)。

    Parameters
    ----------
    strategy_cls:
        nautilus_trader.trading.strategy.Strategy のサブクラス。
    scenario:
        SCENARIO dict。instrument/instruments / granularity / initial_cash を使う。
    bars_by_instrument:
        {InstrumentId: list[Bar]} — catalog_data_loader.load_bars_for_scenario() の戻り値。
        または合成 Bar リストを直接渡してもよい（テスト用）。
    run_buffer:
        RunBufferLike を満たすオブジェクト。fill / equity を受け取る。
    strategy_init_kwargs:
        strategy_cls(**strategy_init_kwargs) で渡す kwargs。None → {}。
    """
    kwargs = strategy_init_kwargs or {}
    granularity = normalize_granularity(scenario["granularity"])
    instruments = instruments_from_scenario(scenario)
    initial_cash = int(scenario.get("initial_cash", 10_000_000))

    # venue は全銘柄共通 (例: "1301.TSE" → "TSE")
    venue_str = InstrumentId.from_str(instruments[0]).venue.value

    cfg = BacktestEngineConfig(
        trader_id="STRATRUNNER-001",
        logging=_BYPASS_LOG,
    )
    assert cfg.cache.database is None, "nautilus persistence must be disabled"

    engine = BacktestEngine(config=cfg)

    fill_handlers: dict[str, object] = {}
    bar_handlers: dict[str, object] = {}

    try:
        # ── Venue 登録 ────────────────────────────────────────────────────────
        engine.add_venue(
            venue=Venue(venue_str),
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=JPY,
            starting_balances=[Money(initial_cash, JPY)],
        )

        # ── Instrument 登録 ───────────────────────────────────────────────────
        # instruments は "1301.TSE" のような完全形なので、Symbol 部分 "1301" のみ渡す。
        for symbol in instruments:
            ticker = InstrumentId.from_str(symbol).symbol.value
            engine.add_instrument(make_equity_instrument(ticker, venue_str))

        # ── Strategy 登録 ─────────────────────────────────────────────────────
        strategy = strategy_cls(**kwargs)
        engine.add_strategy(strategy)

        # ── Fill subscribe: events.fills.{instrument_id} ─────────────────────
        # NautilusTrader の ExecutionEngine が OrderFilled を publish するトピック。
        # (execution/engine.pyx _get_fill_events_topic)
        def _make_fill_handler(iid_str: str):
            def _on_fill(event) -> None:
                try:
                    record: dict = {
                        "instrument_id": iid_str,
                        "side": event.order_side.name,
                        "qty": str(event.last_qty),
                        "price": str(event.last_px),
                        "ts_event_ms": event.ts_event // 1_000_000,
                    }
                    commission_raw = getattr(event, "commission", None)
                    if commission_raw is not None:
                        try:
                            record["commission"] = str(commission_raw.as_decimal())
                        except AttributeError:
                            record["commission"] = str(commission_raw)
                    run_buffer.write_fill(record)
                except Exception:
                    log.warning(
                        "[engine_runner] write_fill failed: instrument=%r", iid_str, exc_info=True
                    )

            return _on_fill

        for symbol in instruments:
            iid_str = symbol
            handler = _make_fill_handler(iid_str)
            fill_handlers[iid_str] = handler
            engine.kernel.msgbus.subscribe(
                topic=f"events.fills.{iid_str}",
                handler=handler,
            )

        # ── Bar subscribe: data.bars.{bar_type} → write_equity ───────────────
        bar_period = _GRANULARITY_TO_BAR_PERIOD.get(granularity)

        if bar_period is not None:
            def _on_bar(bar) -> None:
                try:
                    account = engine.kernel.portfolio.account(Venue(venue_str))
                    if account is not None:
                        balance = account.balance_total(JPY)
                        equity = float(str(balance.as_decimal()))
                    else:
                        equity = float(initial_cash)
                    run_buffer.write_equity(
                        {
                            "ts_event_ms": bar.ts_event // 1_000_000,
                            "equity": equity,
                        }
                    )
                except Exception:
                    log.warning("[engine_runner] write_equity failed", exc_info=True)

            for symbol in instruments:
                bar_type_str = bar_type_for_instrument(symbol, granularity)
                bar_topic = f"data.bars.{bar_type_str}"
                bar_handlers[bar_topic] = _on_bar
                engine.kernel.msgbus.subscribe(topic=bar_topic, handler=_on_bar)

        # ── Streaming loop: 1 bar ずつ処理 ────────────────────────────────────
        items = merge_bars_by_ts(bars_by_instrument)
        log.info(
            "[engine_runner] streaming start: instruments=%r granularity=%r bars=%d",
            instruments,
            granularity,
            len(items),
        )

        import time as _time
        for item in items:
            if run_event is not None:
                run_event.wait()
            engine.add_data([item])
            engine.run(streaming=True)
            engine.clear_data()
            if bar_interval_sec > 0:
                _time.sleep(bar_interval_sec)

        log.info("[engine_runner] streaming complete: bars=%d", len(items))

    finally:
        for iid_str, handler in fill_handlers.items():
            try:
                engine.kernel.msgbus.unsubscribe(
                    topic=f"events.fills.{iid_str}", handler=handler
                )
            except Exception:
                pass
        for bar_topic, handler in bar_handlers.items():
            try:
                engine.kernel.msgbus.unsubscribe(topic=bar_topic, handler=handler)
            except Exception:
                pass
        engine.dispose()
