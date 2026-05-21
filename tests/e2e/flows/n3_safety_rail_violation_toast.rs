//! N3 safety_rail_violation_toast — Safety Rail 違反が Footer トーストに乗ること。
//!
//! `SafetyRailViolation` push が `SafetyToast.active` をセットする。新しい違反は古い
//! トーストを置き換える（最新が勝つ）。
//! 詳細は `tests/e2e/FLOWS.md` の N3 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn n3_safety_rail_violation_toast() {
    let mut h = Harness::new();
    assert!(h.safety_toast().active.is_none());

    h.send_event(BackendEvent::SafetyRailViolation {
        run_id: "r-1".to_string(),
        kind: "MAX_POSITION_SIZE".to_string(),
        detail: "projected position 1200000 JPY exceeds cap 1000000 JPY".to_string(),
        ts_ms: 1000,
    });

    let toast = h.safety_toast().active.expect("violation must surface a toast");
    assert_eq!(toast.kind, "MAX_POSITION_SIZE");
    assert_eq!(toast.run_id, "r-1");
    assert_eq!(toast.ts_ms, 1000);

    // A newer violation supersedes the older toast.
    h.send_event(BackendEvent::SafetyRailViolation {
        run_id: "r-1".to_string(),
        kind: "MAX_DAILY_LOSS".to_string(),
        detail: "daily P&L -150000 JPY breached loss limit -100000 JPY".to_string(),
        ts_ms: 2000,
    });

    let toast = h.safety_toast().active.expect("toast still active");
    assert_eq!(toast.kind, "MAX_DAILY_LOSS", "newer violation must replace the older one");
    assert_eq!(toast.ts_ms, 2000);
}
