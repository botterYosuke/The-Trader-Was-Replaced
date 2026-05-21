//! F5 secret_required — 第二暗証要求でシークレットプロンプトが開くこと。
//!
//! `BackendEvent::SecretRequired` で `SecretPrompt.active` が `Some` になり、
//! request_id / venue / kind / purpose が一致することを確認する（発注時の第二
//! 暗証番号要求）。
//! 詳細は `tests/e2e/FLOWS.md` の F5 を参照。

use crate::support::Harness;
use backcast::trading::BackendEvent;

#[test]
fn f5_secret_required() {
    let mut h = Harness::new();
    assert!(h.secret_prompt().active.is_none());

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
