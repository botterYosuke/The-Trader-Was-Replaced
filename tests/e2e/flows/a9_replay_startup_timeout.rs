//! A9 replay_startup_timeout — first tick が来ないまま60秒経つとソフトタイムアウト error が出ること。
//!
//! arm した起動ウィンドウで headless `Time<Real>` を進め、本番の
//! `replay_startup_timeout_system` を駆動する。59秒では error なし、61秒で
//! `error` がセットされる（phase は不変・ウィンドウは開いたまま＝ユーザーが
//! 読めるように残す）。Close で error がクリアされ hide することも確認する。
//! 詳細は `tests/e2e/FLOWS.md` の A9 を参照。

use crate::support::Harness;
use backcast::replay::ReplayStartupPhase;

#[test]
fn a9_replay_startup_timeout() {
    let mut h = Harness::new();
    h.arm_startup_timeout(42);

    // Just under the threshold: no error yet.
    h.advance_real_time(std::time::Duration::from_secs(59));
    assert!(h.startup_progress().error.is_none(), "must not time out before 60s");
    assert!(h.startup_progress().visible);

    // Past the threshold: soft-timeout error surfaces, window stays open.
    h.advance_real_time(std::time::Duration::from_secs(2));
    let p = h.startup_progress();
    assert!(p.error.is_some(), "soft timeout must set an error after 60s");
    assert!(p.visible, "window stays open so the user can read the error");
    assert_eq!(
        p.phase,
        ReplayStartupPhase::WaitingForFirstTick,
        "timeout annotates with error but does not change the phase"
    );

    // Close dismisses the error and hides the window.
    h.close_startup_window();
    let p = h.startup_progress();
    assert!(!p.visible);
    assert!(p.error.is_none());
}
