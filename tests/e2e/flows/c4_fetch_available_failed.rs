//! C4 fetch_available_failed — Replay 入場の auto-fetch が失敗したとき記録されること。
//!
//! backend 接続済みで scenario.end が決まると本番 `auto_fetch_available_on_replay_entry_system`
//! が `FetchAvailableInstruments` を送る。transport channel で観測した後、backend が
//! `AvailableInstrumentsFetchFailed` を押し戻すと `last_error`（end_date と理由）がセットされ、
//! 当該 end_date の `in_flight` がクリアされることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C4 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand};
use chrono::NaiveDate;

#[test]
fn c4_fetch_available_failed() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

    h.send_status(BackendStatusUpdate::Connected(true));
    h.set_scenario_end("2024-12-31");
    h.tick();
    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|c| matches!(
            c,
            TransportCommand::FetchAvailableInstruments { end_date: d } if *d == end_date
        )),
        "Replay 入場で FetchAvailableInstruments が発射されるはず (got {cmds:?})"
    );

    h.send_status(BackendStatusUpdate::AvailableInstrumentsFetchFailed {
        end_date,
        error: "no catalog".to_string(),
    });

    let a = h.available();
    let (date, err) = a.last_error.expect("last_error set");
    assert_eq!(date, end_date);
    assert_eq!(err, "no catalog");
    assert!(!a.in_flight.contains(&end_date));
}
