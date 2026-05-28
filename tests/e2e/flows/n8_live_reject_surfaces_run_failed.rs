//! N8 live_reject_surfaces_run_failed — gRPC が RegisterLiveStrategy / StartLiveStrategy を
//! reject したとき、`CurrentRun.state` が `RunState::Failed { error }` になることを保証する。
//!
//! Slice 2 において LastRunResult は CurrentRun へ統一された。
//! fix 前（main.rs が error! ログのみで RunFailed を送出しない）は assert fail（RED）になる。
//! 詳細は `tests/e2e/FLOWS.md` の N 群を参照。

use serial_test::serial;

use backcast::backend_sync::{
    apply_status_update, build_register_reject_message, build_start_reject_message,
};
use backcast::trading::{
    AvailableInstruments, BackendStatus, BackendStatusUpdate, CurrentRun, ExecutionModeRes,
    LastPrices, LiveOrders, OrderFeedback, ReconcilePrompt, RunState, SecretPrompt, Tickers,
    VenueStatusRes,
};
use backcast::replay::ReplayStartupProgress;

use tokio::sync::mpsc;

/// `apply_status_update` に `RunFailed` を直接注入し、`CurrentRun.state` が
/// `RunState::Failed { error }` になることを確認する。
///
/// これは Slice 2 の「main.rs の StartLiveAuto ハンドラで gRPC reject → RunFailed 送出」
/// 実装に対する回帰ガード。fix 前は RunFailed が送出されないため、このテストは到達できない
/// （main.rs 側の単体的確認は n8b で行う予定）。
///
/// NOTE: `apply_status_update` 直接呼び出しは配線の正しさを確認しない。
/// main.rs → StatusUpdateChannel → status_update_system → CurrentRun の
/// フル配線は n8b（またはこのテスト内の第二フェーズ）で補完すること。
#[test]
#[serial]
fn n8_grpc_reject_propagates_run_failed_via_apply_status_update() {
    let (tx, _rx) = mpsc::unbounded_channel::<BackendStatusUpdate>();
    let _ = tx; // channel 構築の確認のみ

    // apply_status_update を直接呼んで RunFailed → CurrentRun 経路を確認する。
    let mut status = BackendStatus::default();
    let mut current_run = CurrentRun::default();
    let mut portfolio = Default::default();
    let mut available = AvailableInstruments::default();
    let mut progress = ReplayStartupProgress::default();
    let mut venue_status = VenueStatusRes::default();
    let mut exec_mode = ExecutionModeRes::default();
    let mut tickers = Tickers::default();
    let mut last_prices = LastPrices::default();
    let mut live_orders = LiveOrders::default();
    let mut order_feedback = OrderFeedback::default();
    let mut reconcile_prompt = ReconcilePrompt::default();
    let mut secret_prompt = SecretPrompt::default();

    apply_status_update(
        BackendStatusUpdate::RunFailed {
            startup_id: None,
            error: "RegisterLiveStrategy rejected: invalid credentials".to_string(),
        },
        &mut status,
        &mut current_run,
        &mut portfolio,
        &mut available,
        &mut progress,
        &mut venue_status,
        &mut exec_mode,
        &mut tickers,
        &mut last_prices,
        &mut live_orders,
        &mut order_feedback,
        &mut reconcile_prompt,
        &mut secret_prompt,
    );

    assert!(
        matches!(
            current_run.state,
            RunState::Failed { ref error } if error.contains("RegisterLiveStrategy rejected")
        ),
        "gRPC reject 時に RunFailed が CurrentRun に書かれるはず。実際: {:?}",
        current_run.state,
    );
}

/// n8b: main.rs reject ハンドラの「純関数 build_*_reject_message → RunFailed → CurrentRun.Failed」
/// 配線を real-entity で守る。旧 n8 は RunFailed を手書きするだけで build_* を経由しないため、
/// reject メッセージの整形と「success=true は送らない」契約を検証できなかった。これがその穴埋め。
#[test]
#[serial]
fn n8b_build_reject_message_drives_run_failed_via_channel() {
    // success=false → Some(整形済みメッセージ)。main.rs register 分岐と同じ呼び出し。
    let msg = build_register_reject_message(
        false,
        "STRATEGY_LOAD_FAILED",
        "unexpected indent at line 42",
        "7203.TSE",
        "tachibana",
    )
    .expect("success=false なら Some が返るはず");

    // success=true は送出しない契約（start 側で確認）。
    assert!(
        build_start_reject_message(true, "X", "", "sid", "7203.TSE", "tachibana").is_none(),
        "success=true のときは RunFailed を送らない（None）はず",
    );

    // main.rs → StatusUpdateChannel に相当する unbounded mpsc 経由で配線する。
    let (tx, mut rx) = mpsc::unbounded_channel::<BackendStatusUpdate>();
    tx.send(BackendStatusUpdate::RunFailed {
        startup_id: None,
        error: msg,
    })
    .expect("送信できるはず");

    let update = rx.try_recv().expect("RunFailed が届くはず");

    let mut status = BackendStatus::default();
    let mut current_run = CurrentRun::default();
    let mut portfolio = Default::default();
    let mut available = AvailableInstruments::default();
    let mut progress = ReplayStartupProgress::default();
    let mut venue_status = VenueStatusRes::default();
    let mut exec_mode = ExecutionModeRes::default();
    let mut tickers = Tickers::default();
    let mut last_prices = LastPrices::default();
    let mut live_orders = LiveOrders::default();
    let mut order_feedback = OrderFeedback::default();
    let mut reconcile_prompt = ReconcilePrompt::default();
    let mut secret_prompt = SecretPrompt::default();

    apply_status_update(
        update,
        &mut status,
        &mut current_run,
        &mut portfolio,
        &mut available,
        &mut progress,
        &mut venue_status,
        &mut exec_mode,
        &mut tickers,
        &mut last_prices,
        &mut live_orders,
        &mut order_feedback,
        &mut reconcile_prompt,
        &mut secret_prompt,
    );

    assert!(
        matches!(
            current_run.state,
            RunState::Failed { ref error }
                if error.contains("RegisterLiveStrategy rejected")
                    && error.contains("unexpected indent at line 42")
        ),
        "reject メッセージが CurrentRun.Failed.error に整形済みで届くはず。実際: {:?}",
        current_run.state,
    );
}
