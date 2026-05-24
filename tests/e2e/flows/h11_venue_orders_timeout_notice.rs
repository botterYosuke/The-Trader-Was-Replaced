//! H11 venue_orders_timeout_notice — venue working-orders 取得がタイムアウトしたとき
//! footer の OrderPanel feedback に明示 notice が出る（サイレントに「backend は注文なし」と
//! 扱わせない、issue #29 Slice3b Medium-4）。
//!
//! transport task は GetOrders 応答の error_code が非空なら `get_orders_notice` が返す
//! `OrderNotice` を送る。`apply_status_update` は order_feedback.message に焼く。
//! 詳細は `tests/e2e/FLOWS.md` の H11 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn h11_venue_orders_timeout_notice() {
    let mut h = Harness::new();

    // 取得失敗前は feedback 空。
    assert!(h.order_feedback().message.is_none());

    // GetOrders が timeout（error_code 非空）→ transport task が OrderNotice を送る。
    h.send_status(BackendStatusUpdate::OrderNotice {
        message: "venue の注文取得に失敗しました（VENUE_ORDERS_TIMEOUT）— venue で注文状態を確認してください"
            .to_string(),
    });

    assert_eq!(
        h.order_feedback().message.as_deref(),
        Some("venue の注文取得に失敗しました（VENUE_ORDERS_TIMEOUT）— venue で注文状態を確認してください"),
        "timeout notice が feedback line に出るはず"
    );
}
