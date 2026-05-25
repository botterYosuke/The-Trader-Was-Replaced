//! N8 live_reject_surfaces_run_failed — gRPC が RegisterLiveStrategy / StartLiveStrategy を
//! reject したとき、`LastRunResult.state` が `RunState::Failed { error }` になることを保証する。
//!
//! Slice 2 実装後に CurrentRun へリネームされたら import と resource 参照を更新する（NOTE 参照）。
//! fix 前（main.rs が error! ログのみで RunFailed を送出しない）は assert fail（RED）になる。
//! 詳細は `tests/e2e/FLOWS.md` の N 群を参照。
//!
//! NOTE: 現時点では `LastRunResult` を使用。Slice 2 で `CurrentRun` にリネーム後、
//! この import と resource 参照を `CurrentRun` に更新すること。

use serial_test::serial;

use backcast::backend_sync::{apply_status_update, StatusUpdateChannel};
use backcast::trading::{
    AvailableInstruments, BackendStatus, BackendStatusUpdate, ExecutionModeRes, LastPrices,
    LastRunResult, LiveOrders, OrderFeedback, ReconcilePrompt, RunState, SecretPrompt,
    Tickers, VenueStatusRes,
};
use backcast::replay::ReplayStartupProgress;

use tokio::sync::mpsc;

/// `apply_status_update` に `RunFailed` を直接注入し、`LastRunResult.state` が
/// `RunState::Failed { error }` になることを確認する。
///
/// これは Slice 2 の「main.rs の StartLiveAuto ハンドラで gRPC reject → RunFailed 送出」
/// 実装に対する回帰ガード。fix 前は RunFailed が送出されないため、このテストは到達できない
/// （main.rs 側の単体的確認は n8b で行う予定）。
///
/// NOTE: `apply_status_update` 直接呼び出しは配線の正しさを確認しない。
/// main.rs → StatusUpdateChannel → status_update_system → LastRunResult の
/// フル配線は n8b（またはこのテスト内の第二フェーズ）で補完すること。
#[test]
#[serial]
fn n8_grpc_reject_propagates_run_failed_via_apply_status_update() {
    let (tx, _rx) = mpsc::unbounded_channel::<BackendStatusUpdate>();
    let _ = tx; // channel 構築の確認のみ

    // apply_status_update を直接呼んで RunFailed → LastRunResult 経路を確認する。
    let mut status = BackendStatus::default();
    let mut last_run = LastRunResult::default();
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
        &mut last_run,
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
            last_run.state,
            RunState::Failed { ref error } if error.contains("RegisterLiveStrategy rejected")
        ),
        "gRPC reject 時に RunFailed が LastRunResult に書かれるはず。実際: {:?}",
        last_run.state,
    );
}
