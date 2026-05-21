//! A9 replay_startup_timeout — Run 後 first tick が来ないまま60秒経つとソフトタイムアウト
//! error が出ること。
//!
//! 実 Run ボタンを本番経路で駆動して起動ウィンドウを開き、backend が
//! `WaitingForFirstTick` まで進めた後に first tick が来ない状況を作る。headless
//! `Time<Real>` を進めて本番 `replay_startup_timeout_system` を駆動し、59秒では error
//! なし、61秒で `error` がセットされる（phase は不変・ウィンドウは開いたまま＝ユーザーが
//! 読めるように残す）。Close で error がクリアされ hide することも確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A9 を参照。

use crate::support::Harness;
use backcast::replay::ReplayStartupPhase;
use backcast::trading::{BackendStartupStage, BackendStatusUpdate};

#[test]
fn a9_replay_startup_timeout() {
    let mut h = Harness::new();
    let startup_id = h.run_via_ui();
    h.drain_commands();

    // backend は first tick 待ちまで進むが、その先の tick は来ない。
    h.send_status(BackendStatusUpdate::ReplayStartup {
        startup_id,
        stage: BackendStartupStage::WaitingForFirstTick,
    });
    assert_eq!(
        h.startup_progress().phase,
        ReplayStartupPhase::WaitingForFirstTick
    );

    // 閾値手前: まだ error なし。
    h.advance_real_time(std::time::Duration::from_secs(59));
    assert!(
        h.startup_progress().error.is_none(),
        "must not time out before 60s"
    );
    assert!(h.startup_progress().visible);

    // 閾値超え: ソフトタイムアウト error が出てウィンドウは開いたまま。
    h.advance_real_time(std::time::Duration::from_secs(2));
    let p = h.startup_progress();
    assert!(p.error.is_some(), "soft timeout must set an error after 60s");
    assert!(p.visible, "window stays open so the user can read the error");
    assert_eq!(
        p.phase,
        ReplayStartupPhase::WaitingForFirstTick,
        "timeout annotates with error but does not change the phase"
    );

    // Close で error をクリアしてウィンドウを hide する。
    h.close_startup_window();
    let p = h.startup_progress();
    assert!(!p.visible);
    assert!(p.error.is_none());
}
