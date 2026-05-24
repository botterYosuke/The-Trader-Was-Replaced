//! L7 attach_live_venue_mismatch — `live_venue` が設定された supervisor が既存 backend に attach するとき、
//! backend の `configured_venue` と一致しなければ `StartupFailed("BACKEND_VENUE_MISMATCH")` に、
//! 一致すれば `Ready` に到達することを保証する（kind:integration / mock-server）。
//!
//! ## 背景（issue #24）
//! autospawn を SIGKILL 系で停止すると Python child が孤児化し port 19876 を握り続ける。
//! 孤児が非 live で起動されていた場合、次回 live venue シナリオで supervisor がそのまま attach し
//! `VenueLogin` がサイレントに `LIVE_ADAPTER_NOT_CONFIGURED` を返すという不具合（issue #24）。
//!
//! attach 時に `GetState.configured_venue` と `SupervisorConfig.live_venue` を照合し、
//! 不一致なら `BACKEND_VENUE_MISMATCH` を publish して loud fail させることで解消する。
//!
//! ## 検証点
//! - `live_venue=Some("TACHIBANA")` + backend `configured_venue=None`（孤児ケース）
//!   → `StartupFailed("BACKEND_VENUE_MISMATCH")`
//! - `live_venue=Some("TACHIBANA")` + backend `configured_venue=Some("TACHIBANA")`（正常ケース）
//!   → `Ready`

use std::net::SocketAddr;
use std::time::Duration;

use serde_json::json;
use serial_test::serial;
use tokio::sync::{mpsc, oneshot, watch};
use tonic::{Request, Response, Status, transport::Server};

use backcast::backend_supervisor::{BackendLifecycle, SupervisorConfig, error_code, run_supervisor};
use backcast::trading::engine::{
    BackendEvent, CancelOrderReq, CancelOrderRes, ForceStopReplayRequest, GetLiveStrategyStatusReq,
    GetLiveStrategyStatusRes, GetOrderStatusReq, GetOrderStatusRes, GetOrdersReq, GetOrdersRes,
    GetPortfolioRequest, GetPortfolioResponse, GetStateRequest, GetStateResponse,
    HealthCheckRequest, HealthCheckResponse,
    ListAllListedSymbolsRequest, ListAllListedSymbolsResponse, ListInstrumentsRequest,
    ListInstrumentsResponse, ListLiveStrategiesReq, ListLiveStrategiesRes, LiveStrategyControlRes,
    LoadReplayDataRequest, ModifyOrderReq, ModifyOrderRes, PauseLiveStrategyReq, PauseReplayRequest,
    PlaceOrderReq, PlaceOrderRes, RegisterLiveStrategyReq, RegisterLiveStrategyRes,
    ReplayControlResponse, ResumeLiveStrategyReq, ResumeReplayRequest, SetExecutionModeRequest,
    SetExecutionModeResponse, SetReplaySpeedRequest, ShutdownRequest, ShutdownResponse,
    StartEngineRequest, StartEngineResponse, StartLiveStrategyReq, StartLiveStrategyRes,
    StartRequest, StartResponse, StopEngineRequest, StopLiveStrategyReq, StopReplayRequest,
    StopRequest, StopResponse, SubmitSecretReq, SubmitSecretRes, SubscribeBackendEventsReq,
    SubscribeRequest, SubscribeResponse, UnsubscribeRequest, VenueControlResponse,
    VenueLoginRequest, VenueLoginResponse, VenueLogoutRequest,
    data_engine_server::{DataEngine, DataEngineServer},
    health_check_response::ServingStatus,
    health_server::{Health, HealthServer},
};

// ── 最小 MockHealth（SERVING 固定） ──────────────────────────────────────────

struct MockHealth;

#[tonic::async_trait]
impl Health for MockHealth {
    async fn check(
        &self,
        _req: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: ServingStatus::Serving as i32,
        }))
    }
}

// ── `configured_venue` を静的に返す MockDataEngine ──────────────────────────

struct MockDataEngine {
    token: String,
    configured_venue: Option<String>,
}

#[tonic::async_trait]
impl DataEngine for MockDataEngine {
    async fn get_state(
        &self,
        req: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        if req.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        let state = json!({
            "price": 100.0,
            "history": [],
            "timestamp": 0.0,
            "timestamp_ms": 1_600_000_000_000i64,
            "configured_venue": self.configured_venue,
        });
        Ok(Response::new(GetStateResponse {
            json_data: state.to_string(),
        }))
    }

    // ── DataEngine trait の必須スタブ ──────────────────────────────────���─────

    async fn load_replay_data(&self, _: Request<LoadReplayDataRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn start_engine(&self, _: Request<StartEngineRequest>) -> Result<Response<StartEngineResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn stop_engine(&self, _: Request<StopEngineRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn set_replay_speed(&self, _: Request<SetReplaySpeedRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn pause_replay(&self, _: Request<PauseReplayRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn resume_replay(&self, _: Request<ResumeReplayRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn step_replay(&self, _: Request<backcast::trading::engine::StepReplayRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn stop_replay(&self, _: Request<StopReplayRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn force_stop_replay(&self, _: Request<ForceStopReplayRequest>) -> Result<Response<ReplayControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn start(&self, _: Request<StartRequest>) -> Result<Response<StartResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn stop(&self, _: Request<StopRequest>) -> Result<Response<StopResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn shutdown(&self, _: Request<ShutdownRequest>) -> Result<Response<ShutdownResponse>, Status> {
        Ok(Response::new(ShutdownResponse { accepted: true, error_code: String::new() }))
    }
    async fn list_instruments(&self, _: Request<ListInstrumentsRequest>) -> Result<Response<ListInstrumentsResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn list_all_listed_symbols(&self, _: Request<ListAllListedSymbolsRequest>) -> Result<Response<ListAllListedSymbolsResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn get_portfolio(&self, _: Request<GetPortfolioRequest>) -> Result<Response<GetPortfolioResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn venue_login(&self, _: Request<VenueLoginRequest>) -> Result<Response<VenueLoginResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn venue_logout(&self, _: Request<VenueLogoutRequest>) -> Result<Response<VenueControlResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn subscribe_market_data(&self, _: Request<SubscribeRequest>) -> Result<Response<SubscribeResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn unsubscribe_market_data(&self, _: Request<UnsubscribeRequest>) -> Result<Response<SubscribeResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn set_execution_mode(&self, _: Request<SetExecutionModeRequest>) -> Result<Response<SetExecutionModeResponse>, Status> { Err(Status::unimplemented("stub")) }
    async fn submit_secret(&self, _: Request<SubmitSecretReq>) -> Result<Response<SubmitSecretRes>, Status> { Err(Status::unimplemented("stub")) }
    type SubscribeBackendEventsStream = tokio_stream::Empty<Result<BackendEvent, Status>>;
    async fn subscribe_backend_events(&self, _: Request<SubscribeBackendEventsReq>) -> Result<Response<Self::SubscribeBackendEventsStream>, Status> {
        Ok(Response::new(tokio_stream::empty()))
    }
    async fn place_order(&self, _: Request<PlaceOrderReq>) -> Result<Response<PlaceOrderRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn cancel_order(&self, _: Request<CancelOrderReq>) -> Result<Response<CancelOrderRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn modify_order(&self, _: Request<ModifyOrderReq>) -> Result<Response<ModifyOrderRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn get_order_status(&self, _: Request<GetOrderStatusReq>) -> Result<Response<GetOrderStatusRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn get_orders(&self, _: Request<GetOrdersReq>) -> Result<Response<GetOrdersRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn register_live_strategy(&self, _: Request<RegisterLiveStrategyReq>) -> Result<Response<RegisterLiveStrategyRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn start_live_strategy(&self, _: Request<StartLiveStrategyReq>) -> Result<Response<StartLiveStrategyRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn stop_live_strategy(&self, _: Request<StopLiveStrategyReq>) -> Result<Response<LiveStrategyControlRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn pause_live_strategy(&self, _: Request<PauseLiveStrategyReq>) -> Result<Response<LiveStrategyControlRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn resume_live_strategy(&self, _: Request<ResumeLiveStrategyReq>) -> Result<Response<LiveStrategyControlRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn get_live_strategy_status(&self, _: Request<GetLiveStrategyStatusReq>) -> Result<Response<GetLiveStrategyStatusRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn list_live_strategies(&self, _: Request<ListLiveStrategiesReq>) -> Result<Response<ListLiveStrategiesRes>, Status> { Err(Status::unimplemented("stub")) }
    async fn force_account_snapshot(&self, _: Request<backcast::trading::engine::ForceAccountSnapshotRequest>) -> Result<Response<backcast::trading::engine::ForceAccountSnapshotResponse>, Status> {
        Ok(Response::new(backcast::trading::engine::ForceAccountSnapshotResponse { success: true, error_code: String::new() }))
    }
}

// ── 共通ヘルパー ──────────────────────────────────────────────────────────────

async fn attach_and_await_terminal(config: SupervisorConfig) -> BackendLifecycle {
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (_ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));
    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| {
            matches!(s, BackendLifecycle::Ready | BackendLifecycle::StartupFailed(_))
        }),
    )
    .await
    .expect("supervisor が 5s 以内に終端状態へ到達するはず")
    .expect("watch channel が開いたまま");
    *lr.borrow()
}

// ── テスト本体 ────────────────────────────────────────────────────────────────

/// L7a: 孤児ケース。`live_venue=Some("TACHIBANA")` で attach したとき backend の
/// `configured_venue` が None（非 live 孤児）なら `StartupFailed(BACKEND_VENUE_MISMATCH)` になる。
#[tokio::test]
#[serial]
async fn l7a_attach_live_venue_mismatch_reaches_venue_mismatch() {
    let addr: SocketAddr = "127.0.0.1:50082".parse().unwrap();
    let token = "l7-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let engine = MockDataEngine {
        token: token.clone(),
        configured_venue: None,
    };
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(HealthServer::new(MockHealth))
            .add_service(DataEngineServer::new(engine))
            .serve_with_shutdown(addr, async { rx_close.await.ok(); })
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
        live_venue: Some("TACHIBANA".to_string()),
    };

    let outcome = attach_and_await_terminal(config).await;

    assert_eq!(
        outcome,
        BackendLifecycle::StartupFailed(error_code::VENUE_MISMATCH),
        "非 live 孤児への attach は BACKEND_VENUE_MISMATCH で失敗するはず (got {outcome:?})"
    );

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

/// L7b: 正常ケース。`live_venue=Some("TACHIBANA")` で attach したとき backend の
/// `configured_venue` が一致すれば `Ready` になる。
#[tokio::test]
#[serial]
async fn l7b_attach_live_venue_match_reaches_ready() {
    let addr: SocketAddr = "127.0.0.1:50083".parse().unwrap();
    let token = "l7-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let engine = MockDataEngine {
        token: token.clone(),
        configured_venue: Some("TACHIBANA".to_string()),
    };
    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(HealthServer::new(MockHealth))
            .add_service(DataEngineServer::new(engine))
            .serve_with_shutdown(addr, async { rx_close.await.ok(); })
            .await
            .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
        live_venue: Some("TACHIBANA".to_string()),
    };

    let outcome = attach_and_await_terminal(config).await;

    assert_eq!(
        outcome,
        BackendLifecycle::Ready,
        "live venue 一致の attach は Ready になるはず (got {outcome:?})"
    );

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}
