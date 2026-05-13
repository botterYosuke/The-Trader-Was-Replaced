import pytest
import json
import time
import math
from pydantic import ValidationError
from engine.models import HistoryPoint, TradingState, EngineSnapshot
from engine.core import DataEngine
from engine.replay import SimpleCSVProvider

def test_history_point_validation():
    # Valid
    hp = HistoryPoint(timestamp_ms=1600000000000, price=120.5)
    assert hp.timestamp_ms == 1600000000000
    assert hp.price == 120.5

    # Invalid price (non-finite)
    with pytest.raises(ValidationError):
        HistoryPoint(timestamp_ms=1600000000000, price=float('nan'))
    with pytest.raises(ValidationError):
        HistoryPoint(timestamp_ms=1600000000000, price=float('inf'))

    # Invalid timestamp (<= 0)
    with pytest.raises(ValidationError):
        HistoryPoint(timestamp_ms=0, price=120.5)

def test_trading_state_validation():
    # Valid
    ts = TradingState(
        price=100.0,
        history=[90.0, 100.0],
        timestamp=1600000000.0,
        timestamp_ms=1600000000000,
        history_points=[
            HistoryPoint(timestamp_ms=1599999999000, price=90.0),
            HistoryPoint(timestamp_ms=1600000000000, price=100.0)
        ]
    )
    assert ts.timestamp_ms == 1600000000000
    assert len(ts.history_points) == 2

    # history validation (non-finite)
    with pytest.raises(ValidationError):
        TradingState(price=100.0, history=[float('nan')], timestamp=1600000000.0)

def test_data_engine_max_history_len():
    # max_history_len = 5
    engine = DataEngine(max_history_len=5)
    engine.start()
    
    # Static mode initially has 4 points
    state = engine.get_current_state()
    assert len(state.history) == 4
    assert len(state.history_points) == 4
    
    # Advance twice -> 6 points total -> should be capped at 5
    engine.advance()
    engine.advance()
    
    state = engine.get_current_state()
    assert len(state.history) == 5
    assert len(state.history_points) == 5

def test_data_engine_replay_timestamp_normalization(tmp_path):
    csv = tmp_path / "test.csv"
    # Unix timestamp in SECONDS
    csv.write_text("timestamp,price\n1600000000,100.0\n1600000005,105.0\n")
    
    provider = SimpleCSVProvider(str(csv))
    engine = DataEngine(replay_provider=provider)
    
    # Primed state
    state = engine.get_current_state()
    assert state.price == 100.0
    assert state.timestamp == 1600000000.0
    assert state.timestamp_ms == 1600000000000 # Normalized to ms
    assert state.history_points[0].timestamp_ms == 1600000000000
    
    # Advance
    engine.start()
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 105.0
    assert state.timestamp == 1600000005.0
    assert state.timestamp_ms == 1600000005000
    assert state.history_points[1].timestamp_ms == 1600000005000

def test_trading_state_json_dump():
    ts = TradingState(
        price=100.0,
        history=[100.0],
        timestamp=1600000000.0,
        timestamp_ms=1600000000000,
        history_points=[HistoryPoint(timestamp_ms=1600000000000, price=100.0)]
    )
    json_str = ts.model_dump_json()
    data = json.loads(json_str)
    
    assert data["price"] == 100.0
    assert data["timestamp_ms"] == 1600000000000
    assert len(data["history_points"]) == 1
    assert data["history_points"][0]["timestamp_ms"] == 1600000000000
    assert data["history_points"][0]["price"] == 100.0

def test_data_engine_snapshot_compatibility():
    # Phase 4 snapshot might not have history_points/timestamp_ms
    engine = DataEngine()
    
    # Old snapshot without history_points
    # TradingState requires history_points if passed, but we can omit it if it has default
    # Looking at models.py, history_points has default_factory=list
    old_state = TradingState(
        price=150.0,
        history=[140.0, 150.0],
        timestamp=1700000000.0
    )
    
    snapshot = EngineSnapshot(
        state=old_state,
        replay_index=0,
        mode="static"
    )
    
    # Should restore and reconstruct history_points
    engine.restore_snapshot(snapshot)
    state = engine.get_current_state()
    assert state.price == 150.0
    assert state.timestamp_ms == 1700000000000
    assert len(state.history_points) == 2
    # Check reconstruction
    assert state.history_points[1].timestamp_ms == 1700000000000
    assert state.history_points[1].price == 150.0
    assert state.history_points[0].timestamp_ms == 1700000000000 - 1000
    assert state.history_points[0].price == 140.0
