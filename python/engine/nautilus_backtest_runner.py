"""NautilusBacktestRunner — BacktestEngine streaming runner for GUI (issue #68 Slice 1).

Mirrors engine_runner.py streaming approach but delivers each bar to
RustBacktestSink.push_bar() so the Rust/Bevy side can render OHLC in real-time.

Public API:
    NautilusBacktestRunner(*, catalog_path, strategy_file, instruments,
                           start_date, end_date, granularity,
                           initial_cash, rust_sink)
    .run() -> {"success": bool, "run_id": str, "error": str}
"""
from __future__ import annotations

import logging
from typing import Any

log = logging.getLogger(__name__)


class NautilusBacktestRunner:
    """Runs a strategy via BacktestEngine and streams bars to RustBacktestSink.

    Instruments / date range from the caller override the SCENARIO embedded in the
    strategy file (issue #68: Instrument Registry wins over SCENARIO).
    """

    def __init__(
        self,
        *,
        catalog_path: str,
        strategy_file: str,
        instruments: list[str],
        start_date: str = "",
        end_date: str = "",
        granularity: str = "Daily",
        initial_cash: float = 10_000_000.0,
        rust_sink: Any,
    ) -> None:
        self._catalog_path = catalog_path
        self._strategy_file = strategy_file
        self._instruments = instruments
        self._start_date = start_date
        self._end_date = end_date
        self._granularity = granularity
        self._initial_cash = int(initial_cash)
        self._rust_sink = rust_sink

    def run(self) -> dict:
        """Execute the backtest synchronously.

        Returns {"success": bool, "run_id": str, "error": str}.
        Bars stream to rust_sink.push_bar() as they are processed.
        push_run_complete() is called once on success.
        """
        from engine.strategy_runtime import strategy_loader
        from engine.strategy_runtime.catalog_data_loader import (
            bar_type_for_instrument,
            load_bars_for_scenario,
            merge_bars_by_ts,
            normalize_granularity,
        )
        from engine.strategy_runtime.instrument_factory import make_equity_instrument
        from engine.live.gui_bridge_actor import GuiBridgeActor

        from nautilus_trader.backtest.engine import BacktestEngine
        from nautilus_trader.config import BacktestEngineConfig, LoggingConfig
        from nautilus_trader.model.currencies import JPY
        from nautilus_trader.model.enums import AccountType, OmsType
        from nautilus_trader.model.identifiers import InstrumentId, Venue
        from nautilus_trader.model.objects import Money

        # --- Load strategy file -----------------------------------------------
        try:
            _module, scenario, strategy_cls = strategy_loader.load(self._strategy_file)
        except Exception as exc:
            return {"success": False, "run_id": "", "error": f"strategy load failed: {exc}"}

        # Instrument Registry overrides SCENARIO (issue #68 design)
        if self._instruments:
            scenario["instruments"] = list(self._instruments)
        if self._start_date:
            scenario["start"] = self._start_date
        if self._end_date:
            scenario["end"] = self._end_date

        try:
            granularity = normalize_granularity(self._granularity)
        except ValueError as exc:
            return {"success": False, "run_id": "", "error": str(exc)}

        # --- Load bars from catalog --------------------------------------------
        try:
            bars_by_instrument = load_bars_for_scenario(self._catalog_path, scenario)
        except Exception as exc:
            return {"success": False, "run_id": "", "error": f"catalog load failed: {exc}"}

        instruments = list(scenario["instruments"])
        venue_str = InstrumentId.from_str(instruments[0]).venue.value

        # --- Build BacktestEngine ---------------------------------------------
        cfg = BacktestEngineConfig(
            trader_id="GUIRUNNER-001",
            logging=LoggingConfig(bypass_logging=True),
        )
        engine = BacktestEngine(config=cfg)
        bar_handlers: dict[str, Any] = {}

        try:
            engine.add_venue(
                venue=Venue(venue_str),
                oms_type=OmsType.NETTING,
                account_type=AccountType.CASH,
                base_currency=JPY,
                starting_balances=[Money(self._initial_cash, JPY)],
            )
            for symbol in instruments:
                ticker = InstrumentId.from_str(symbol).symbol.value
                engine.add_instrument(make_equity_instrument(ticker, venue_str))

            strategy = strategy_cls()
            engine.add_strategy(strategy)

            # Subscribe GuiBridgeActor to bar events via msgbus
            bridge = GuiBridgeActor(self._rust_sink, instrument_id="")
            bar_handler = bridge.make_bar_handler()
            for symbol in instruments:
                bar_type_str = bar_type_for_instrument(symbol, granularity)
                bar_topic = f"data.bars.{bar_type_str}"
                bar_handlers[bar_topic] = bar_handler
                engine.kernel.msgbus.subscribe(topic=bar_topic, handler=bar_handler)

            # --- Streaming loop: 1 bar at a time ------------------------------
            items = merge_bars_by_ts(bars_by_instrument)
            log.info(
                "[NautilusBacktestRunner] start: instruments=%r granularity=%r bars=%d",
                instruments,
                granularity,
                len(items),
            )

            for item in items:
                engine.add_data([item])
                engine.run(streaming=True)
                engine.clear_data()

            log.info("[NautilusBacktestRunner] complete: bars=%d", len(items))
            self._rust_sink.push_run_complete("", "{}")
            return {"success": True, "run_id": "", "error": ""}

        except Exception as exc:
            log.error("[NautilusBacktestRunner] run failed: %s", exc, exc_info=True)
            return {"success": False, "run_id": "", "error": str(exc)}
        finally:
            for topic, handler in bar_handlers.items():
                try:
                    engine.kernel.msgbus.unsubscribe(topic=topic, handler=handler)
                except Exception:
                    pass
            try:
                engine.dispose()
            except Exception:
                pass
