import pytest
import os
import time
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
    
    # Next advance should be the third tick
    new_engine.advance()
    state = new_engine.get_current_state()
    assert state.price == 102.0
    assert state.timestamp == 1600000002

def test_static_mode():
    engine = DataEngine() # No replay provider
    engine.start()
    
    initial_price = engine.get_current_state().price
    engine.advance()
    new_price = engine.get_current_state().price
    
    assert initial_price != new_price
    assert len(engine.get_current_state().history) > 4 # Initial history was 4
