//! A8 stale_startup_id_ignored — 古い startup_id の完了通知で起動ウィンドウを閉じないこと。
//!
//! 相関 ID ロジックの回帰ガード。古い `startup_id` を持つ `RunComplete` では
//! 起動ウィンドウは開いたまま、現在の id に一致する `RunComplete` でのみ閉じる
//! ことを確認する（superseded された旧 run の遅延通知でウィンドウを誤って
//! 閉じない）。
//! 詳細は `tests/e2e/FLOWS.md` の A8 を参照。

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

#[test]
fn a8_stale_startup_id_ignored() {
    let mut h = Harness::new();
    h.begin_startup(9);

    // Stale completion from a superseded run — window must stay open.
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(8),
        run_id: "stale".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(h.startup_progress().visible, "stale id closed the window");

    // Matching completion closes the window.
    h.send_status(BackendStatusUpdate::RunComplete {
        startup_id: Some(9),
        run_id: "current".to_string(),
        summary_json: r#"{"status":"ok"}"#.to_string(),
    });
    assert!(!h.startup_progress().visible, "matching id did not close window");
}
