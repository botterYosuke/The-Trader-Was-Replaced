//! F5 secret_required — 発注時の第二暗証要求でシークレットプロンプトが開くこと。
//!
//! Manual モードの注文フォームを本番経路で駆動して `PlaceOrder` を送ると、Tachibana では
//! backend が `BackendEvent::SecretRequired` を返す。これで `SecretPrompt.active` が `Some` に
//! なり、request_id / venue / kind / purpose が一致することを確認する。
//! 詳細は `tests/e2e/FLOWS.md` の F5 を参照。

use crate::support::Harness;
use backcast::trading::{BackendEvent, TransportCommand};

#[test]
fn f5_secret_required() {
    let mut h = Harness::new();
    assert!(h.secret_prompt().active.is_none());

    // 実注文フォームで発注 → PlaceOrder コマンド。
    let cmds = h.place_order_via_ui("7203.TSE");
    assert!(
        cmds.iter()
            .any(|c| matches!(c, TransportCommand::PlaceOrder { .. })),
        "[発注]→[Confirm] は PlaceOrder を送るはず (got {cmds:?})"
    );

    // backend が第二暗証番号を要求する。
    h.send_event(BackendEvent::SecretRequired {
        request_id: "req-1".to_string(),
        venue: "tachibana".to_string(),
        kind: "second_password".to_string(),
        purpose: "place_order".to_string(),
    });

    let req = h.secret_prompt().active.expect("prompt active");
    assert_eq!(req.request_id, "req-1");
    assert_eq!(req.venue, "tachibana");
    assert_eq!(req.kind, "second_password");
    assert_eq!(req.purpose, "place_order");
}
