//! N5 promote_result_feedback — Promote to Live の結果が PromoteFeedback に出ること。
//!
//! `LiveStrategyPromoteResult` status seam が `PromoteFeedback.message` をセットする。
//! 成功時は新 run id を含む起動メッセージ、拒否時は error_code を含む拒否メッセージ。
//! OrderFeedback（LiveManual 専用）ではなく PromoteFeedback に出るのは、promote の結果が
//! LiveAuto でも見える必要があるため。
//! 詳細は `tests/e2e/FLOWS.md` の N5 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn n5_promote_result_feedback() {
    let mut h = Harness::new();
    assert!(h.promote_feedback().message.is_none());

    // Reject: the structured error_code must be visible to the user.
    h.send_status(BackendStatusUpdate::LiveStrategyPromoteResult {
        success: false,
        error_code: "VENUE_LOGIN_REQUIRED".to_string(),
        run_id: String::new(),
    });
    let msg = h.promote_feedback().message.expect("reject must surface feedback");
    assert!(msg.contains("VENUE_LOGIN_REQUIRED"), "reject must name the error_code: {msg}");

    // Success: the message names the new Live run id.
    h.send_status(BackendStatusUpdate::LiveStrategyPromoteResult {
        success: true,
        error_code: String::new(),
        run_id: "live-run-9".to_string(),
    });
    let msg = h.promote_feedback().message.expect("success must surface feedback");
    assert!(msg.contains("live-run-9"), "success must name the new run id: {msg}");
}
