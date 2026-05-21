//! C3 fetch_available_instruments — Replay 入場で利用可能銘柄が auto-fetch され、
//! 指定 end_date の取得が成功すること。
//!
//! backend 接続済みで scenario.end が決まると、本番 `auto_fetch_available_on_replay_entry_system`
//! が `TransportCommand::FetchAvailableInstruments{end_date}` を送る（その間 in_flight が立つ）。
//! transport channel でコマンドを観測した後、backend が `AvailableInstrumentsLoaded` を押し戻すと
//! `AvailableInstruments.by_end_date[end_date]` が充填され in_flight がクリアされることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C3 を参照。

use crate::support::Harness;
use backcast::trading::{BackendStatusUpdate, TransportCommand};
use chrono::NaiveDate;

#[test]
fn c3_fetch_available_instruments() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

    // 接続済み + scenario.end 決定 → Replay 入場 auto-fetch がコマンドを発射する。
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
    assert!(
        h.available().in_flight.contains(&end_date),
        "fetch 中は in_flight マーカーが立つ"
    );

    // backend が応答する。
    h.send_status(BackendStatusUpdate::AvailableInstrumentsLoaded {
        end_date,
        ids: vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
    });

    let a = h.available();
    assert_eq!(a.by_end_date.get(&end_date).map(|v| v.len()), Some(2));
    assert!(!a.in_flight.contains(&end_date));
    assert!(a.last_error.is_none());
}
