//! A8 stale_startup_id_ignored — 現在の run と一致しない startup_id の完了通知では
//! 起動ウィンドウを閉じないこと。
//!
//! 相関 ID ロジックの回帰ガード。実 Run ボタンを本番経路で駆動して起動ウィンドウを開き
//! startup_id を割り当てた後、一致しない `startup_id` を持つ `RunComplete` では起動
//! ウィンドウは開いたまま、現在の id に一致する `RunComplete` でのみ閉じることを確認する
//! （superseded された別 run の遅延通知でウィンドウを誤って閉じない）。
//! 詳細は `tests/e2e/FLOWS.md` の A8 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn a8_stale_startup_id_ignored() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();
    h.drain_commands();
    assert!(h.startup_progress().visible, "Run で起動ウィンドウが開くはず");

    // 現在の run と一致しない完了通知 — ウィンドウは開いたまま。
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(startup_id.wrapping_add(1)),
        run_id: "stale".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(
        h.startup_progress().visible,
        "一致しない id がウィンドウを閉じてしまった"
    );

    // 一致する完了通知でウィンドウが閉じる。
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(startup_id),
        run_id: "current".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(
        !h.startup_progress().visible,
        "一致する id でウィンドウが閉じなかった"
    );
}
