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

def test_trading_state_execution_mode_default_is_replay():
    state = TradingState(price=100.0, history=[90.0])
    assert state.execution_mode == "Replay"

def test_trading_state_execution_mode_accepts_all_three_values():
    for mode in ("Replay", "LiveManual", "LiveAuto"):
        state = TradingState(price=100.0, history=[90.0], execution_mode=mode)
        assert state.execution_mode == mode
    with pytest.raises(ValidationError):
        TradingState(price=100.0, history=[90.0], execution_mode="Live")

def test_trading_state_venue_fields_default_none_and_empty():
    state = TradingState(price=100.0, history=[90.0])
    assert state.venue_state is None
    assert state.venue_id is None
    assert state.subscribed_instruments == []

def test_trading_state_venue_fields_roundtrip():
    state = TradingState(
        price=100.0,
        history=[90.0],
        venue_state="CONNECTED",
        venue_id="TACHIBANA",
        subscribed_instruments=["1301.TSE", "5401.TSE"],
    )
    json_data = state.model_dump_json()
    import json
    data = json.loads(json_data)
    assert data["venue_state"] == "CONNECTED"
    assert data["venue_id"] == "TACHIBANA"
    assert data["subscribed_instruments"] == ["1301.TSE", "5401.TSE"]

def test_trading_state_extra_forbid_still_holds_for_new_payload():
    with pytest.raises(ValidationError):
        TradingState(
            price=100.0,
            history=[90.0],
            execution_mode="LiveManual",
            venue_state="CONNECTED",
            venue_id="TACHIBANA",
            subscribed_instruments=["1301.TSE"],
            foo=1,
        )
