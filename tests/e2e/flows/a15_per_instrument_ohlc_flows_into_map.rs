//! A15 — `per_instrument` OHLC seam: BackendTradingState → InstrumentTradingDataMap (issue #57).

use crate::support::Harness;
use backcast::trading::{BackendTradingState, InstrumentTradingDataMap};
use serde_json::json;

#[test]
fn a15_per_instrument_ohlc_flows_into_map() {
    let mut h = Harness::new();

    // Initially the map is empty.
    assert!(
        h.app
            .world()
            .resource::<InstrumentTradingDataMap>()
            .map
            .is_empty(),
        "InstrumentTradingDataMap should be empty before any state push"
    );

    // Simulate backend sending first step: 2 OHLC bars for "AAPL.NASDAQ".
    let state: BackendTradingState = serde_json::from_value(json!({
        "price": 110.0,
        "history": [],
        "timestamp": 1.0,
        "timestamp_ms": 10_000,
        "per_instrument": {
            "AAPL.NASDAQ": {
                "price": 110.0,
                "ohlc_points": [
                    {
                        "timestamp_ms": 5_000, "open_time_ms": 5_000,
                        "open": 100.0, "high": 105.0, "low": 95.0, "close": 100.0
                    },
                    {
                        "timestamp_ms": 10_000, "open_time_ms": 10_000,
                        "open": 100.0, "high": 115.0, "low": 100.0, "close": 110.0
                    }
                ]
            }
        }
    }))
    .expect("BackendTradingState fixture");
    h.backend_tx.send(state).expect("backend channel");
    h.tick();

    {
        let map = h.app.world().resource::<InstrumentTradingDataMap>();
        let data = map
            .map
            .get("AAPL.NASDAQ")
            .expect("per_instrument[AAPL.NASDAQ] must be in map after push_state (RED: empty if #57 unfixed)");
        assert_eq!(
            data.ohlc_points.len(),
            2,
            "first push: 2 ohlc_points expected (issue #57)"
        );
        assert_eq!(
            data.ohlc_points.last().unwrap().open_time_ms,
            10_000,
            "latest bar open_time_ms should be 10_000"
        );
    }

    // Simulate second step: 3 bars now.
    let state2: BackendTradingState = serde_json::from_value(json!({
        "price": 120.0,
        "history": [],
        "timestamp": 1.0,
        "timestamp_ms": 15_000,
        "per_instrument": {
            "AAPL.NASDAQ": {
                "price": 120.0,
                "ohlc_points": [
                    {
                        "timestamp_ms": 5_000, "open_time_ms": 5_000,
                        "open": 100.0, "high": 105.0, "low": 95.0, "close": 100.0
                    },
                    {
                        "timestamp_ms": 10_000, "open_time_ms": 10_000,
                        "open": 100.0, "high": 115.0, "low": 100.0, "close": 110.0
                    },
                    {
                        "timestamp_ms": 15_000, "open_time_ms": 15_000,
                        "open": 110.0, "high": 125.0, "low": 108.0, "close": 120.0
                    }
                ]
            }
        }
    }))
    .expect("BackendTradingState fixture 2");
    h.backend_tx.send(state2).expect("backend channel");
    h.tick();

    let map = h.app.world().resource::<InstrumentTradingDataMap>();
    let data = map
        .map
        .get("AAPL.NASDAQ")
        .expect("per_instrument[AAPL.NASDAQ] must still be in map after second push");
    assert_eq!(
        data.ohlc_points.len(),
        3,
        "second push: 3 ohlc_points expected"
    );
    assert_eq!(data.ohlc_points.last().unwrap().open_time_ms, 15_000);
}
