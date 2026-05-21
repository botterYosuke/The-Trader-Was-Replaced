"""Unit tests for the strategy → UI log helper (Phase 10 §570 remediation).

`emit_strategy_log` must (1) mirror the line to Nautilus's structured log and
(2) publish a `StrategyLogRecord` on `strategy.log.{strategy_id}` — without ever
crashing the strategy if the publish fails. These use plain fakes (no kernel).
"""

from __future__ import annotations

import pytest

from engine.live.strategy_log import (
    StrategyLogRecord,
    emit_strategy_log,
    strategy_log_topic,
)


class _FakeLog:
    def __init__(self) -> None:
        self.calls: list[tuple[str, str]] = []

    def debug(self, m: str) -> None:
        self.calls.append(("debug", m))

    def info(self, m: str) -> None:
        self.calls.append(("info", m))

    def warning(self, m: str) -> None:
        self.calls.append(("warning", m))

    def error(self, m: str) -> None:
        self.calls.append(("error", m))


class _FakeBus:
    def __init__(self) -> None:
        self.published: list[tuple[str, object]] = []

    def publish(self, topic, msg):
        self.published.append((topic, msg))


class _FakeClock:
    def timestamp_ns(self) -> int:
        return 1_700_000_000_000_000_000


class _FakeStrategy:
    def __init__(self) -> None:
        self.log = _FakeLog()
        self.msgbus = _FakeBus()
        self.clock = _FakeClock()
        self.id = "LIVE-abcd1234"


def test_topic_is_prefix_plus_strategy_id():
    assert strategy_log_topic("LIVE-abcd1234") == "strategy.log.LIVE-abcd1234"


def test_emit_mirrors_to_log_and_publishes_record():
    s = _FakeStrategy()
    emit_strategy_log(s, "entering long", "INFO")
    # (1) mirrored to the structured log.
    assert s.log.calls == [("info", "entering long")]
    # (2) published on the per-run topic with a well-formed record.
    assert len(s.msgbus.published) == 1
    topic, rec = s.msgbus.published[0]
    assert topic == "strategy.log.LIVE-abcd1234"
    assert isinstance(rec, StrategyLogRecord)
    assert rec.level == "INFO"
    assert rec.message == "entering long"
    assert rec.ts_ns == 1_700_000_000_000_000_000


def test_emit_levels_map_case_insensitively():
    s = _FakeStrategy()
    emit_strategy_log(s, "w", "warning")
    emit_strategy_log(s, "e", "ERROR")
    assert s.log.calls == [("warning", "w"), ("error", "e")]
    assert [rec.level for _, rec in s.msgbus.published] == ["WARNING", "ERROR"]


def test_emit_normalises_unknown_level_to_info():
    s = _FakeStrategy()
    emit_strategy_log(s, "x", "trace")
    assert s.log.calls == [("info", "x")]
    assert s.msgbus.published[0][1].level == "INFO"


def test_emit_never_raises_when_publish_fails():
    s = _FakeStrategy()

    class _Boom:
        def publish(self, *a):
            raise RuntimeError("bus down")

    s.msgbus = _Boom()
    # Must not raise — logging is best-effort. The structured-log mirror still runs.
    emit_strategy_log(s, "still logged", "INFO")
    assert s.log.calls == [("info", "still logged")]
