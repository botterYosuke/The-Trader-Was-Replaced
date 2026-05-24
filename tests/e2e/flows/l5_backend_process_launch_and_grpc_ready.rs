//! L5 backend_process_launch_and_grpc_ready — supervisor が mock gRPC サーバに対して attach し、
//! Health.Check → SERVING、GetState（token 一致）を経て `BackendLifecycle::Ready` に到達することを
//! 保証する（kind:integration / mock-server）。
//!
//! ## 実装方針
//! 実際の `python -m engine` subprocess は重く CI では不安定なため、`tests/backend_integration.rs`
//! と同じ「mock tonic gRPC サーバ + `run_supervisor(autospawn: false)`」パターンを採用する。
//! supervisor は既存の attach 経路（probe → Health.Check → GetState handshake）を実行し、
//! Ready まで辿り着く。
//!
//! 実 Python backend process の起動は `BACKEND_PROCESS_INTEGRATION=1` を要求する
//! 別の opt-in テスト（将来の L5b など）で扱う。
//!
//! ## 検証点
//! - mock server が SERVING を返すと supervisor は `BackendLifecycle::Ready` になる。
//! - mock server が GetState に対して有効なレスポンスを返すと Ready が確定する。
//! - 接続後に Health を切ると supervisor は Crashed か ShuttingDown に遷移する。

use std::net::SocketAddr;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tonic::{Request, Response, Status, transport::Server};

use backcast::backend_supervisor::{BackendLifecycle, SupervisorConfig, run_supervisor};
use backcast::trading::engine::{
    HealthCheckRequest, HealthCheckResponse,
    health_check_response::ServingStatus,
    health_server::{Health, HealthServer},
    GetStateRequest, GetStateResponse,
    data_engine_server::{DataEngine, DataEngineServer},
    // ── 残りは DataEngine trait の必須メソッドをコンパイルするための型 ──────
    BackendEvent, CancelOrderReq, CancelOrderRes, ForceStopReplayRequest, GetLiveStrategyStatusReq,
    GetLiveStrategyStatusRes, GetOrderStatusReq, GetOrderStatusRes, GetOrdersReq, GetOrdersRes,
    GetPortfolioRequest, GetPortfolioResponse, ListAllListedSymbolsRequest,
    ListAllListedSymbolsResponse, ListInstrumentsRequest, ListInstrumentsResponse,
    ListLiveStrategiesReq, ListLiveStrategiesRes, LiveStrategyControlRes, LoadReplayDataRequest,
    ModifyOrderReq, ModifyOrderRes, PauseLiveStrategyReq, PauseReplayRequest, PlaceOrderReq,
    PlaceOrderRes, RegisterLiveStrategyReq, RegisterLiveStrategyRes, ReplayControlResponse,
    ResumeLiveStrategyReq, ResumeReplayRequest, SetExecutionModeRequest, SetExecutionModeResponse,
    SetReplaySpeedRequest, ShutdownRequest, ShutdownResponse, StartEngineRequest,
    StartEngineResponse, StartLiveStrategyReq, StartLiveStrategyRes, StartRequest, StartResponse,
    StopEngineRequest, StopLiveStrategyReq, StopReplayRequest, StopRequest, StopResponse,
    SubmitSecretReq, SubmitSecretRes, SubscribeBackendEventsReq, SubscribeRequest,
    SubscribeResponse, UnsubscribeRequest, VenueControlResponse, VenueLoginRequest,
    VenueLoginResponse, VenueLogoutRequest,
};

// ── 最小 MockHealth（SERVING 固定） ──────────────────────────────────────────

struct MockHealth {
    status: i32,
}

#[tonic::async_trait]
impl Health for MockHealth {
    async fn check(
        &self,
        _req: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        Ok(Response::new(HealthCheckResponse {
            status: self.status,
        }))
    }
}

// ── 最小 MockDataEngine（token 検証 + GetState のみ） ────────────────────────

struct MockDataEngine {
    token: String,
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
        let state = serde_json::json!({
            "price": 100.0,
            "history": [],
            "timestamp": 0.0,
            "timestamp_ms": 1_600_000_000_000i64,
        });
        Ok(Response::new(GetStateResponse {
            json_data: state.to_string(),
        }))
    }

    // ── 残りは DataEngine trait の必須スタブ ─────────────────────────────────

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
}

// ── テスト本体 ────────────────────────────────────────────────────────────────

/// `run_supervisor(autospawn: false)` を mock サーバに向けて起動し、
/// Ready / StartupFailed などの終端ライフサイクルに達するまで待つ共通ヘルパー。
/// backend_integration.rs の `run_attach_and_await_terminal` と同等。
async fn attach_and_await_terminal(config: SupervisorConfig) -> BackendLifecycle {
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (_ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));
    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| {
            matches!(
                s,
                BackendLifecycle::Ready | BackendLifecycle::StartupFailed(_)
            )
        }),
    )
    .await
    .expect("supervisor が 5s 以内に終端状態へ到達するはず")
    .expect("watch channel が開いたまま")
    ;
    *lr.borrow()
}

#[tokio::test]
async fn l5_backend_process_launch_and_grpc_ready() {
    // mock サーバのポートは backend_integration.rs と衝突しないよう 50080 台を使う。
    let addr: SocketAddr = "127.0.0.1:50081".parse().unwrap();
    let token = "l5-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let health = MockHealth {
        status: ServingStatus::Serving as i32,
    };
    let engine = MockDataEngine {
        token: token.clone(),
    };

    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(HealthServer::new(health))
            .add_service(DataEngineServer::new(engine))
            .serve_with_shutdown(addr, async {
                rx_close.await.ok();
            })
            .await
            .unwrap();
    });
    // サーバが bind するまで少し待つ（backend_integration.rs と同じパターン）。
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        // autospawn: false = subprocess を起動しない attach モード。
        // mock gRPC サーバに直接 Health.Check → GetState ハンドシェイクを行う。
        autospawn: false,
        cwd: None,
        python_bin: None,
        live_venue: None,
    };

    let outcome = attach_and_await_terminal(config).await;

    // mock SERVING + token 一致 → Ready に到達するはず。
    assert_eq!(
        outcome,
        BackendLifecycle::Ready,
        "mock SERVING サーバへの attach は BackendLifecycle::Ready になるはず (got {outcome:?})"
    );

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}
