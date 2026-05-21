//! C3 fetch_available_instruments — 指定 end_date の利用可能銘柄取得が成功すること。
//!
//! `AvailableInstrumentsLoaded` で `AvailableInstruments.by_end_date[end_date]`
//! が充填され、当該 end_date の `in_flight` マーカーがクリアされることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C3 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;
use chrono::NaiveDate;

#[test]
fn c3_fetch_available_instruments() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

    h.send_status(BackendStatusUpdate::AvailableInstrumentsLoaded {
        end_date,
        ids: vec!["1301.TSE".to_string(), "7203.TSE".to_string()],
    });

    let a = h.available();
    assert_eq!(a.by_end_date.get(&end_date).map(|v| v.len()), Some(2));
    assert!(!a.in_flight.contains(&end_date));
    assert!(a.last_error.is_none());
}
