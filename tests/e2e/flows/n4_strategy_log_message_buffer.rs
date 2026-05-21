//! N4 strategy_log_message_buffer — 戦略ログ行が StrategyLogs リングバッファに溜まること。
//!
//! `StrategyLogMessage` push が `StrategyLogs` に oldest-first で積まれ、`recent(n)` が
//! 直近 n 行を時系列順で返す。バッファは `CAP`（100）行で頭から切り捨てられる。
//! 詳細は `tests/e2e/FLOWS.md` の N4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, StrategyLogs};

#[test]
fn n4_strategy_log_message_buffer() {
    let mut h = Harness::new();
    assert!(h.strategy_logs().lines.is_empty());

    for (i, level) in [(1, "INFO"), (2, "WARN"), (3, "ERROR")] {
        h.send_event(BackendEvent::StrategyLogMessage {
            run_id: "r-1".to_string(),
            level: level.to_string(),
            message: format!("line {i}"),
            ts_ms: 1000 * i as i64,
        });
    }

    let logs = h.strategy_logs();
    assert_eq!(logs.lines.len(), 3);
    // recent(2) returns the last two, oldest-first (chronological top→bottom).
    let recent: Vec<_> = logs.recent(2).map(|l| l.message.clone()).collect();
    assert_eq!(recent, vec!["line 2".to_string(), "line 3".to_string()]);

    // The ring buffer caps at StrategyLogs::CAP, dropping the oldest.
    for i in 0..(StrategyLogs::CAP + 5) {
        h.send_event(BackendEvent::StrategyLogMessage {
            run_id: "r-1".to_string(),
            level: "INFO".to_string(),
            message: format!("flood {i}"),
            ts_ms: 10_000 + i as i64,
        });
    }
    let logs = h.strategy_logs();
    assert_eq!(logs.lines.len(), StrategyLogs::CAP, "buffer must cap at CAP lines");
    assert_eq!(
        logs.lines.back().map(|l| l.message.clone()),
        Some(format!("flood {}", StrategyLogs::CAP + 4)),
        "newest line is retained at the back"
    );
}
