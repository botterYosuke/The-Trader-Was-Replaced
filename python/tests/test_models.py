import pytest
from pydantic import ValidationError
from engine.models import TradingState
import time

def test_trading_state_valid():
    state = TradingState(price=100.0, history=[90.0, 95.0, 100.0])
    assert state.price == 100.0
    assert state.history == [90.0, 95.0, 100.0]
    assert isinstance(state.timestamp, float)
    assert state.timestamp <= time.time()

def test_trading_state_strict_type():
    with pytest.raises(ValidationError):
        TradingState(price="100.0", history=[90.0])

def test_trading_state_extra_forbid():
    with pytest.raises(ValidationError):
        TradingState(price=100.0, history=[90.0], timer=42.0)

def test_trading_state_missing_field():
    with pytest.raises(ValidationError):
        TradingState(history=[90.0])

def test_trading_state_negative_price():
    with pytest.raises(ValidationError):
        TradingState(price=-1.0, history=[90.0])

def test_trading_state_nan_inf():
    with pytest.raises(ValidationError):
        TradingState(price=float("nan"), history=[90.0])
    with pytest.raises(ValidationError):
        TradingState(price=float("inf"), history=[90.0])

def test_trading_state_history_non_finite():
    with pytest.raises(ValidationError):
        TradingState(price=100.0, history=[90.0, float("nan")])
    with pytest.raises(ValidationError):
        TradingState(price=100.0, history=[90.0, float("inf")])

def test_trading_state_serialization():
    state = TradingState(price=100.0, history=[90.0, 95.0], timestamp=123456789.0)
    json_data = state.model_dump_json()
    import json
    data = json.loads(json_data)
    assert data["price"] == 100.0
    assert data["history"] == [90.0, 95.0]
    assert data["timestamp"] == 123456789.0
    assert "timer" not in data
