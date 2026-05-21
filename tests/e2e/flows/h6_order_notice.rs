//! H6 order_notice — 構造化拒否でない注文 notice が verbatim 表示されること。
//!
//! `OrderNotice{message}` が `OrderFeedback.message` にそのまま（整形せず）出る
//! ことを確認する。incomplete success / transport error など、構造化 reject では
//! ない注文フローの通知を扱う。
//! 詳細は `tests/e2e/FLOWS.md` の H6 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h6_order_notice() {
    let mut h = Harness::new();
    assert!(h.order_feedback().message.is_none());

    h.send_status(BackendStatusUpdate::OrderNotice {
        message: "注文は受け付けられましたが状態を追跡できません".to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("注文は受け付けられましたが状態を追跡できません")
    );
}
