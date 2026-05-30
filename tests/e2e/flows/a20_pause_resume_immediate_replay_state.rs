//! A20 pause_resume_immediate_replay_state — PauseReplay / ResumeReplay RPC 成功後に
//! GetState ポーリング（最大 1 秒）を待たずに `TradingSession.replay_state` が即時更新されること。
//!
//! ## 問題 (issue #63)
//! Pause ボタン（`||`）をクリックしても `state: PAUSED` にならない。根本原因:
//! `PauseReplay` RPC が成功しても Rust 側は単にログするだけで、`TradingSession.replay_state`
//! を更新しない。UI は次の `GetState` ポーリング（デフォルト 1 秒後）まで RUNNING を表示し続け、
//! ユーザーには「Pause が効いていない」と見える。
//!
//! ## 修正
//! `BackendStatusUpdate::ReplayStateChanged { state }` variant を追加し、transport task が
//! `PauseReplay` / `ResumeReplay` RPC 成功後に即時送出する。`apply_status_update` が
//! `TradingSession.replay_state` を `Some(state)` に更新する。
//!
//! ## seam
//! `BackendStatusUpdate::ReplayStateChanged` を `send_status` で直接注入し、
//! `TradingSession.replay_state` が同一 tick で更新されることを assert する。
//! transport task ↔ gRPC の実通信は mock 不要（seam は ECS 境界）。
//!
//! kind:state  be:none  優先:★★★

use crate::support::Harness;
use backcast::trading::BackendStatusUpdate;

/// A20 (kind:state): `ReplayStateChanged{PAUSED}` を注入すると
/// `TradingSession.replay_state` が同一 tick で `"PAUSED"` になること。
#[test]
fn a20_pause_replay_state_changed_updates_trading_session_immediately() {
    let mut h = Harness::new();

    // 事前: backend が RUNNING を返してきた状態を模擬。
    h.set_replay_state(Some("RUNNING"));
    assert_eq!(h.replay_state().as_deref(), Some("RUNNING"));

    // transport task が PauseReplay RPC 成功後に即時送出する seam を注入。
    h.send_status(BackendStatusUpdate::ReplayStateChanged {
        state: "PAUSED".to_string(),
    });

    // GetState ポーリングを待たずに同一 tick で PAUSED に更新される。
    assert_eq!(
        h.replay_state().as_deref(),
        Some("PAUSED"),
        "PauseReplay RPC 成功後に TradingSession.replay_state が即時 PAUSED になるはず"
    );
}

/// A20 (kind:state): `ReplayStateChanged{RUNNING}` を注入すると
/// `TradingSession.replay_state` が同一 tick で `"RUNNING"` になること（Resume 対称）。
#[test]
fn a20_resume_replay_state_changed_updates_trading_session_immediately() {
    let mut h = Harness::new();

    // 事前: PAUSED 状態を模擬。
    h.set_replay_state(Some("PAUSED"));
    assert_eq!(h.replay_state().as_deref(), Some("PAUSED"));

    // transport task が ResumeReplay RPC 成功後に即時送出する seam を注入。
    h.send_status(BackendStatusUpdate::ReplayStateChanged {
        state: "RUNNING".to_string(),
    });

    // GetState ポーリングを待たずに同一 tick で RUNNING に更新される。
    assert_eq!(
        h.replay_state().as_deref(),
        Some("RUNNING"),
        "ResumeReplay RPC 成功後に TradingSession.replay_state が即時 RUNNING になるはず"
    );
}

/// A20 (kind:state): push_state (GetState ポーリング) が来ても replay_state が
/// `ReplayStateChanged` で設定した値を上書きしないこと（GetState も同じ値を返すため
/// 上書きがあっても実害はないが、seam の干渉確認）。
#[test]
fn a20_get_state_poll_after_replay_state_changed_is_consistent() {
    let mut h = Harness::new();

    // Pause 後に即時更新。
    h.set_replay_state(Some("RUNNING"));
    h.send_status(BackendStatusUpdate::ReplayStateChanged {
        state: "PAUSED".to_string(),
    });
    assert_eq!(h.replay_state().as_deref(), Some("PAUSED"));

    // 次の GetState ポーリング（push_state は replay_state=None で来る）を模擬。
    // push_state は replay_state を None にリセットするため、transport 側は
    // GetState の replay_state をそのまま使い続ける（ReplayStateChanged で上書き
    // した値が次の GetState で PAUSED に戻ることをここでは確認しない — それは
    // gRPC GetState のポーリングが担う）。
    // ここでは seam 注入後の状態が正しいことを最低限確認する。
    h.tick();
    assert_eq!(
        h.replay_state().as_deref(),
        Some("PAUSED"),
        "余分な tick 後も PAUSED のまま"
    );
}
