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

def test_replay_priming(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    
    # After init, it should be primed with the FIRST tick
    state = engine.get_current_state()
    assert state.price == 100.0
    assert state.timestamp == 1600000000
    assert state.history == [100.0]
    assert not engine.is_exhausted

def test_replay_progression(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()

    # Initial state is the first tick (primed)
    state = engine.get_current_state()
    assert state.price == 100.0

    # First advance (gets the second tick)
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 101.0
    assert state.timestamp == 1600000001

    # Second advance (gets the third tick)
    engine.advance()
    state = engine.get_current_state()
    assert state.price == 102.0
    assert state.timestamp == 1600000002
    assert engine.is_exhausted # provider says exhausted after returning the last tick

def test_snapshot_restore(csv_file):
    provider = SimpleCSVProvider(csv_file)
    engine = DataEngine(replay_provider=provider)
    engine.start()

    # Already primed at price 100
    engine.advance() # price=101, index=2 in provider
    
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

def test_snapshot_mode_mismatch(csv_file):
    provider = SimpleCSVProvider(csv_file)
    replay_engine = DataEngine(replay_provider=provider)
    static_engine = DataEngine()
    
    replay_engine.start()
    snapshot = replay_engine.take_snapshot()
    
    static_engine.start()
    with pytest.raises(ValueError, match="Snapshot mode mismatch"):
        static_engine.restore_snapshot(snapshot)

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

def test_static_mode_auto_advance_compatibility():
    # Static mode should still start with default price for Phase 1/2 tests
    engine = DataEngine()
    state = engine.get_current_state()
    assert state.price == 120.5
