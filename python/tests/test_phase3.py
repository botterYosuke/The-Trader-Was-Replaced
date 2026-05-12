import pytest
import os
import time
import math
from engine.core import DataEngine
from engine.replay import SimpleCSVProvider
from engine.models import EngineSnapshot, TradingState

@pytest.fixture
def csv_file(tmp_path):
    d = tmp_path / "data"
    d.mkdir()
    f = d / "test.csv"
    f.write_text("timestamp,price\n1600000000,100.0\n1600000001,101.0\n1600000002,102.0\n")
    return str(f)

def test_replay_progression(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()

    # Initial state (should be default before first advance)
    state = engine.get_current_state()
    assert state.price == 120.5 # Default in __init__

    # First advance
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 100.0
    assert state.timestamp == 1600000000

    # Second advance
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 101.0
    assert state.timestamp == 1600000001

    # Third advance
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 102.0
    assert state.timestamp == 1600000002

    # Fourth advance (exhausted)
    engine.advance()
    assert engine.is_exhausted
    state = engine.get_current_state()
    assert state.price == 102.0 # Maintains last price

def test_snapshot_restore(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()

    # Advance to second tick
    engine.advance()
    engine.advance()
    
    snapshot = engine.take_snapshot()
    assert snapshot.replay_index == 2
    assert snapshot.state.price == 101.0

    # Create a new engine with same provider
    new_provider = SimpleCSVProvider(csv_file)
    new_engine = DataEngine(replay_provider=new_provider)
    new_engine.start()
    
    new_engine.restore_snapshot(snapshot)
    
    state = new_engine.get_current_state()
    assert state.price == 101.0
    assert not new_engine.is_exhausted
    
    # Next advance should be the third tick
    new_engine.advance()
    state = new_engine.get_current_state()
    assert state.price == 102.0
    assert state.timestamp == 1600000002
    assert new_engine.is_exhausted

def test_snapshot_restore_to_start(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()
    
    engine.advance() # price=100
    snapshot = engine.take_snapshot()
    
    engine.advance() # price=101
    assert engine.get_current_state().price == 101.0
    
    # Restore to index 1
    engine.restore_snapshot(snapshot)
    assert engine.get_current_state().price == 100.0
    assert engine._replay_provider.current_index == 1

def test_snapshot_source_mismatch(csv_file, tmp_path):
    # Create another CSV
    other_f = tmp_path / "other.csv"
    other_f.write_text("timestamp,price\n2600000000,200.0\n")
    
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()
    engine.advance()
    snapshot = engine.take_snapshot()
    
    other_provider = SimpleCSVProvider(str(other_f))
    other_engine = DataEngine(replay_provider=other_provider)
    other_engine.start()
    
    with pytest.raises(ValueError, match="Snapshot source mismatch"):
        other_engine.restore_snapshot(snapshot)

def test_invalid_csv_data(tmp_path):
    f = tmp_path / "invalid.csv"
    # price <= 0, nan, header
    f.write_text("ts,price\n1600,0\n1601,nan\n1602,100\n")
    
    provider = SimpleCSVProvider(str(f))
    assert len(provider._data) == 1
    assert provider._data[0] == (1602.0, 100.0)

def test_missing_csv():
    with pytest.raises(FileNotFoundError):
        SimpleCSVProvider("non_existent.csv")

def test_empty_csv(tmp_path):
    f = tmp_path / "empty.csv"
    f.write_text("timestamp,price\n")
    with pytest.raises(ValueError, match="No valid data found"):
        SimpleCSVProvider(str(f))

def test_static_mode():
    engine = DataEngine() # No replay provider
    engine.start()
    
    initial_price = engine.get_current_state().price
    # In my current server_grpc.py, static mode doesn't auto-advance.
    # But engine.advance() should still work if called manually.
    engine.advance()
    new_price = engine.get_current_state().price
    
    assert initial_price != new_price
    assert len(engine.get_current_state().history) > 4 
