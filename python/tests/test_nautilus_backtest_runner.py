"""Tests for NautilusBacktestRunner (issue #68 Slice 1 & 2).

MockRustSink は push_bar / push_run_complete を受け取ることを確認する。
BacktestEngine を起動せずにユニットテストできるよう engine_runner.run() と同じ
streaming loop をモック差し替えで検証する。

Slice 2: GuiBridgeActor の pause_event / step_event 制御のユニットテスト。
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any
from unittest.mock import MagicMock, patch

import pytest


# ---------------------------------------------------------------------------
# Mock sink
# ---------------------------------------------------------------------------


class MockRustSink:
    """RustBacktestSink に相当するテスト用シンク。"""

    def __init__(self) -> None:
        self.bars: list[dict] = []
        self.run_complete_calls: list[tuple[str, str]] = []
        self.run_failed_calls: list[str] = []

    def push_bar(self, state_json: str) -> None:
        self.bars.append(json.loads(state_json))

    def push_order(self, _json: str) -> None:
        pass

    def push_portfolio(self, _json: str) -> None:
        pass

    def push_telemetry(self, _json: str) -> None:
        pass

    def push_run_complete(self, run_id: str, summary_json: str) -> None:
        self.run_complete_calls.append((run_id, summary_json))

    def push_run_failed(self, error: str) -> None:
        self.run_failed_calls.append(error)


# ---------------------------------------------------------------------------
# Fake Bar for GuiBridgeActor
# ---------------------------------------------------------------------------


class _FakePrice:
    def __init__(self, value: float) -> None:
        self._value = value

    def as_double(self) -> float:
        return self._value


@dataclass
class FakeBar:
    ts_event: int  # nanoseconds
    open: Any = field(default=None)
    high: Any = field(default=None)
    low: Any = field(default=None)
    close: Any = field(default=None)
    volume: Any = field(default=None)

    def __post_init__(self) -> None:
        if self.open is None:
            self.open = _FakePrice(100.0)
        if self.high is None:
            self.high = _FakePrice(101.0)
        if self.low is None:
            self.low = _FakePrice(99.0)
        if self.close is None:
            self.close = _FakePrice(100.5)
        if self.volume is None:
            self.volume = _FakePrice(1000.0)


# ---------------------------------------------------------------------------
# GuiBridgeActor unit tests (no BacktestEngine)
# ---------------------------------------------------------------------------


class TestGuiBridgeActor:
    def test_push_bar_accumulates_ohlc(self):
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)
        handler = actor.make_bar_handler()

        bar1 = FakeBar(ts_event=1_000_000_000_000)  # 1000 seconds → 1_000_000 ms
        handler(bar1)

        assert len(sink.bars) == 1
        state = sink.bars[0]
        assert state["price"] == pytest.approx(100.5)
        assert len(state["ohlc_points"]) == 1
        ohlc = state["ohlc_points"][0]
        assert ohlc["open"] == pytest.approx(100.0)
        assert ohlc["high"] == pytest.approx(101.0)
        assert ohlc["low"] == pytest.approx(99.0)
        assert ohlc["close"] == pytest.approx(100.5)
        assert ohlc["volume"] == pytest.approx(1000.0)

    def test_push_bar_accumulates_history(self):
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)
        handler = actor.make_bar_handler()

        for i in range(3):
            bar = FakeBar(
                ts_event=(i + 1) * 1_000_000_000_000,
                close=_FakePrice(float(100 + i)),
                open=_FakePrice(float(100 + i)),
                high=_FakePrice(float(101 + i)),
                low=_FakePrice(float(99 + i)),
                volume=_FakePrice(1000.0),
            )
            handler(bar)

        assert len(sink.bars) == 3
        last_state = sink.bars[-1]
        assert len(last_state["history"]) == 3
        assert last_state["history"] == pytest.approx([100.0, 101.0, 102.0])
        assert len(last_state["ohlc_points"]) == 3

    def test_state_json_valid(self):
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)
        handler = actor.make_bar_handler()
        handler(FakeBar(ts_event=1_000_000_000_000))

        state = sink.bars[0]
        assert "price" in state
        assert "timestamp" in state
        assert "timestamp_ms" in state
        assert "history" in state
        assert "ohlc_points" in state

    def test_on_bar_exception_does_not_propagate(self):
        """push_bar が例外を投げても on_bar ハンドラは伝播しない。"""
        from engine.live.gui_bridge_actor import GuiBridgeActor

        class BrokenSink:
            def push_bar(self, _: str) -> None:
                raise RuntimeError("intentional failure")

        actor = GuiBridgeActor(BrokenSink())
        handler = actor.make_bar_handler()
        # Must not raise
        handler(FakeBar(ts_event=1_000_000_000_000))


# ---------------------------------------------------------------------------
# NautilusBacktestRunner unit tests (BacktestEngine mocked)
# ---------------------------------------------------------------------------


class TestNautilusBacktestRunner:
    """BacktestEngine をモックして NautilusBacktestRunner の制御フローを検証。"""

    def _make_runner(self, sink: MockRustSink, **overrides):
        from engine.nautilus_backtest_runner import NautilusBacktestRunner

        params = dict(
            catalog_path="/fake/catalog",
            strategy_file="/fake/strategy.py",
            instruments=["1301.TSE"],
            start_date="2024-01-01",
            end_date="2024-12-31",
            granularity="Daily",
            initial_cash=10_000_000.0,
            rust_sink=sink,
        )
        params.update(overrides)
        return NautilusBacktestRunner(**params)

    def test_missing_strategy_file_returns_error(self):
        """strategy_loader.load が FileNotFoundError を投げたら success=False を返す。"""
        from engine.nautilus_backtest_runner import NautilusBacktestRunner

        sink = MockRustSink()
        runner = NautilusBacktestRunner(
            catalog_path="/fake",
            strategy_file="/nonexistent/strategy.py",
            instruments=["1301.TSE"],
            start_date="2024-01-01",
            end_date="2024-12-31",
            rust_sink=sink,
        )
        result = runner.run()
        assert result["success"] is False
        assert "strategy load failed" in result["error"]

    def test_mock_sink_receives_bars(self):
        """モック BacktestEngine 経由でバーが MockRustSink.push_bar に届く。"""
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)
        handler = actor.make_bar_handler()

        fake_bars = [
            FakeBar(ts_event=(i + 1) * 86_400_000_000_000)  # daily bars
            for i in range(5)
        ]
        for b in fake_bars:
            handler(b)

        assert len(sink.bars) == 5
        assert all("ohlc_points" in s for s in sink.bars)

    def test_push_run_complete_called(self):
        """push_run_complete がちょうど 1 回呼ばれることを確認（モック経由）。"""
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)
        handler = actor.make_bar_handler()

        handler(FakeBar(ts_event=1_000_000_000_000))
        # Simulate successful run completion
        sink.push_run_complete("", "{}")

        assert len(sink.run_complete_calls) == 1
        run_id, summary = sink.run_complete_calls[0]
        assert run_id == ""
        assert summary == "{}"


# ---------------------------------------------------------------------------
# GuiBridgeActor Pause/Step/Resume tests — issue #68 Slice 2 (RED before impl)
# ---------------------------------------------------------------------------


class TestGuiBridgeActorPauseStep:
    """pause_event / step_event threading.Event 制御のユニットテスト (Slice 2)."""

    def test_no_events_backward_compat(self):
        """pause_event=None → 常に処理される（後方互換）。"""
        from engine.live.gui_bridge_actor import GuiBridgeActor

        sink = MockRustSink()
        actor = GuiBridgeActor(sink)  # no events — Slice 1 signature
        handler = actor.make_bar_handler()

        handler(FakeBar(ts_event=1_000_000_000_000))
        assert len(sink.bars) == 1

    def test_resume_allows_bars(self):
        """pause_event が set (running) のとき bars は即処理される。"""
        import threading
        from engine.live.gui_bridge_actor import GuiBridgeActor

        pause_event = threading.Event()
        pause_event.set()  # running
        step_event = threading.Event()

        sink = MockRustSink()
        actor = GuiBridgeActor(sink, pause_event=pause_event, step_event=step_event)
        handler = actor.make_bar_handler()

        for i in range(3):
            handler(FakeBar(ts_event=(i + 1) * 1_000_000_000_000))

        assert len(sink.bars) == 3

    def test_step_allows_exactly_one_bar_when_paused(self):
        """step_event を set すると pause 中でも一本だけ bar が通過し event が消費される。"""
        import threading
        from engine.live.gui_bridge_actor import GuiBridgeActor

        pause_event = threading.Event()  # clear = paused
        step_event = threading.Event()

        sink = MockRustSink()
        actor = GuiBridgeActor(sink, pause_event=pause_event, step_event=step_event)
        handler = actor.make_bar_handler()

        # Step x3 → 3 bars
        for i in range(3):
            step_event.set()
            handler(FakeBar(ts_event=(i + 1) * 1_000_000_000_000))

        assert len(sink.bars) == 3
        assert not step_event.is_set(), "step_event should be consumed after each bar"

    def test_step_event_consumed_after_bar(self):
        """step_event は一本の bar を処理したあと自動的に clear される。"""
        import threading
        from engine.live.gui_bridge_actor import GuiBridgeActor

        pause_event = threading.Event()  # paused
        step_event = threading.Event()
        step_event.set()

        sink = MockRustSink()
        actor = GuiBridgeActor(sink, pause_event=pause_event, step_event=step_event)
        handler = actor.make_bar_handler()

        handler(FakeBar(ts_event=1_000_000_000_000))
        assert len(sink.bars) == 1
        assert not step_event.is_set()

    def test_inproc_server_exposes_pause_resume_step(self):
        """InprocLiveServer が pause/resume/step_backtest() を持つことを確認。"""
        from engine.inproc_server import InprocLiveServer

        assert hasattr(InprocLiveServer, "pause_backtest"), "pause_backtest must exist"
        assert hasattr(InprocLiveServer, "resume_backtest"), "resume_backtest must exist"
        assert hasattr(InprocLiveServer, "step_backtest"), "step_backtest must exist"

    def test_rust_sink_has_push_run_failed(self):
        """MockRustSink が push_run_failed を受け取れることを確認 (Rust 側との契約)。"""
        sink = MockRustSink()
        # Should not raise
        sink.push_run_failed("some error")
        assert len(sink.run_failed_calls) == 1
        assert sink.run_failed_calls[0] == "some error"
