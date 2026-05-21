//! C4 fetch_available_failed — 利用可能銘柄取得の失敗が記録されること。
//!
//! `AvailableInstrumentsFetchFailed` で `last_error`（end_date と理由）が
//! セットされ、当該 end_date の `in_flight` がクリアされることを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の C4 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;
use chrono::NaiveDate;

#[test]
fn c4_fetch_available_failed() {
    let mut h = Harness::new();
    let end_date = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();

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
