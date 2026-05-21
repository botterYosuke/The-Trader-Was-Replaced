use backcast::backend_supervisor::{
    BackendLifecycle, SupervisorCommand, SupervisorConfig, run_supervisor,
};
use backcast::trading::engine::{
    BackendEvent, CancelOrderReq, CancelOrderRes, EngineState, ForceStopReplayRequest,
    GetOrderStatusReq, GetOrderStatusRes, GetOrdersReq, GetOrdersRes, GetPortfolioRequest,
    GetPortfolioResponse, GetStateRequest, GetStateResponse, ListAllListedSymbolsRequest,
    ListAllListedSymbolsResponse, ListInstrumentsRequest, ListInstrumentsResponse,
    LoadReplayDataRequest, ModifyOrderReq, ModifyOrderRes, PauseReplayRequest, PlaceOrderReq,
    PlaceOrderRes, ReplayControlResponse, ResumeReplayRequest, SetExecutionModeRequest,
    SetExecutionModeResponse, SetReplaySpeedRequest, ShutdownRequest, ShutdownResponse,
    StartEngineRequest, StartEngineResponse, StartResponse, StepReplayRequest, StopEngineRequest,
    StopReplayRequest, StopRequest, StopResponse, SubmitSecretReq, SubmitSecretRes,
    SubscribeBackendEventsReq, SubscribeRequest, SubscribeResponse, UnsubscribeRequest,
    VenueControlResponse, VenueLoginRequest, VenueLoginResponse, VenueLogoutRequest,
    data_engine_server::{DataEngine, DataEngineServer},
};
use backcast::trading::engine::{
    HealthCheckRequest, HealthCheckResponse,
    health_check_response::ServingStatus,
    health_server::{Health, HealthServer},
};
use backcast::trading::{BackendTradingState, StartRequest};
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};
use tonic::{Request, Response, Status, transport::Server};

// Mock gRPC server implementation
#[derive(Default)]
pub struct MyDataEngine {
    pub token: String,
    pub running: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// Minimal Health servicer for supervisor attach-path tests. Always returns the
/// configured `status` (a `ServingStatus` int code) for every Check call.
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

#[tonic::async_trait]
impl DataEngine for MyDataEngine {
    async fn load_replay_data(
        &self,
        request: Request<LoadReplayDataRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Loaded as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn start_engine(
        &self,
        request: Request<StartEngineRequest>,
    ) -> Result<Response<StartEngineResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(StartEngineResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Running as i32,
            error_code: None,
            error_message: None,
            run_id: Some("test-run".to_string()),
            summary_json: Some("{}".to_string()),
        }))
    }

    async fn stop_engine(
        &self,
        request: Request<StopEngineRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Stopping as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn set_replay_speed(
        &self,
        request: Request<SetReplaySpeedRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Running as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn pause_replay(
        &self,
        request: Request<PauseReplayRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Paused as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn resume_replay(
        &self,
        request: Request<ResumeReplayRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Running as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn step_replay(
        &self,
        request: Request<StepReplayRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Running as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn stop_replay(
        &self,
        request: Request<StopReplayRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Stopping as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn force_stop_replay(
        &self,
        request: Request<ForceStopReplayRequest>,
    ) -> Result<Response<ReplayControlResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ReplayControlResponse {
            success: true,
            request_id: request.request_id,
            current_state: EngineState::Stopping as i32,
            error_code: "".to_string(),
            error_message: "".to_string(),
        }))
    }

    async fn get_state(
        &self,
        request: Request<GetStateRequest>,
    ) -> Result<Response<GetStateResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }

        let state = json!({
            "price": 123.45,
            "history": [120.0, 123.45],
            "timestamp": 1600000000.0,
            "timestamp_ms": 1600000000000i64,
            "history_points": [
                {"timestamp_ms": 1599999999000i64, "price": 120.0},
                {"timestamp_ms": 1600000000000i64, "price": 123.45}
            ]
        });

        Ok(Response::new(GetStateResponse {
            json_data: state.to_string(),
        }))
    }

    async fn start(
        &self,
        request: Request<StartRequest>,
    ) -> Result<Response<StartResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(Response::new(StartResponse { success: true }))
    }

    async fn stop(&self, request: Request<StopRequest>) -> Result<Response<StopResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(Response::new(StopResponse { success: true }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        Ok(Response::new(ShutdownResponse {
            accepted: true,
            error_code: String::new(),
        }))
    }

    async fn list_instruments(
        &self,
        request: Request<ListInstrumentsRequest>,
    ) -> Result<Response<ListInstrumentsResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ListInstrumentsResponse {
            success: true,
            instrument_ids: vec![],
            error_message: "".to_string(),
            instruments: vec![],
        }))
    }

    async fn list_all_listed_symbols(
        &self,
        request: Request<ListAllListedSymbolsRequest>,
    ) -> Result<Response<ListAllListedSymbolsResponse>, Status> {
        let request = request.into_inner();
        if request.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ListAllListedSymbolsResponse {
            success: true,
            instrument_ids: vec![],
            error_message: String::new(),
            resolved_end_date: request.end_date,
        }))
    }

    async fn get_portfolio(
        &self,
        request: Request<GetPortfolioRequest>,
    ) -> Result<Response<GetPortfolioResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(GetPortfolioResponse {
            success: true,
            buying_power: 0.0,
            cash: 0.0,
            equity: 0.0,
            positions: vec![],
            orders: vec![],
            error_message: "".to_string(),
        }))
    }

    async fn venue_login(
        &self,
        request: Request<VenueLoginRequest>,
    ) -> Result<Response<VenueLoginResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(VenueLoginResponse {
            success: true,
            error_code: "".to_string(),
            venue_state: "CONNECTED".to_string(),
            instruments_loaded: 0,
        }))
    }

    async fn venue_logout(
        &self,
        request: Request<VenueLogoutRequest>,
    ) -> Result<Response<VenueControlResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(VenueControlResponse {
            success: true,
            error_code: "".to_string(),
        }))
    }

    async fn subscribe_market_data(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<SubscribeResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(SubscribeResponse {
            success: false,
            error_code: "NOT_IMPLEMENTED".to_string(),
        }))
    }

    async fn unsubscribe_market_data(
        &self,
        request: Request<UnsubscribeRequest>,
    ) -> Result<Response<SubscribeResponse>, Status> {
        if request.into_inner().token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(SubscribeResponse {
            success: false,
            error_code: "NOT_IMPLEMENTED".to_string(),
        }))
    }

    async fn set_execution_mode(
        &self,
        request: Request<SetExecutionModeRequest>,
    ) -> Result<Response<SetExecutionModeResponse>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(SetExecutionModeResponse {
            success: true,
            error_code: "".to_string(),
            execution_mode: req.mode,
        }))
    }

    async fn submit_secret(
        &self,
        request: Request<SubmitSecretReq>,
    ) -> Result<Response<SubmitSecretRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        let _ = req.request_id;
        let _ = req.secret;
        Ok(Response::new(SubmitSecretRes {
            success: true,
            error_code: "".to_string(),
        }))
    }

    type SubscribeBackendEventsStream = tokio_stream::Empty<Result<BackendEvent, Status>>;

    async fn subscribe_backend_events(
        &self,
        _request: Request<SubscribeBackendEventsReq>,
    ) -> Result<Response<Self::SubscribeBackendEventsStream>, Status> {
        Ok(Response::new(tokio_stream::empty()))
    }

    async fn place_order(
        &self,
        request: Request<PlaceOrderReq>,
    ) -> Result<Response<PlaceOrderRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(PlaceOrderRes {
            success: true,
            error_code: "".to_string(),
            order_event: None,
        }))
    }

    async fn cancel_order(
        &self,
        request: Request<CancelOrderReq>,
    ) -> Result<Response<CancelOrderRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(CancelOrderRes {
            success: true,
            error_code: "".to_string(),
            order_event: None,
        }))
    }

    async fn modify_order(
        &self,
        request: Request<ModifyOrderReq>,
    ) -> Result<Response<ModifyOrderRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(ModifyOrderRes {
            success: true,
            error_code: "".to_string(),
            order_event: None,
        }))
    }

    async fn get_order_status(
        &self,
        request: Request<GetOrderStatusReq>,
    ) -> Result<Response<GetOrderStatusRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(GetOrderStatusRes {
            success: false,
            error_code: "UNKNOWN_ORDER_ID".to_string(),
            order_event: None,
        }))
    }

    async fn get_orders(
        &self,
        request: Request<GetOrdersReq>,
    ) -> Result<Response<GetOrdersRes>, Status> {
        let req = request.into_inner();
        if req.token != self.token {
            return Err(Status::unauthenticated("Invalid token"));
        }
        Ok(Response::new(GetOrdersRes {
            success: true,
            error_code: String::new(),
            orders: vec![],
        }))
    }
}

#[tokio::test]
async fn test_backend_connection_flow() {
    let addr: SocketAddr = "[::1]:50053".parse().unwrap();
    let token = "test-token".to_string();
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let (tx_close, rx_close) = oneshot::channel::<()>();

    let engine = MyDataEngine {
        token: token.clone(),
        running: running.clone(),
    };

    let server_handle = tokio::spawn(async move {
        Server::builder()
            .add_service(DataEngineServer::new(engine))
            .serve_with_shutdown(addr, async {
                rx_close.await.ok();
            })
            .await
            .unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let url = format!("http://{}", addr);
    let mut client =
        backcast::trading::engine::data_engine_client::DataEngineClient::connect(url.clone())
            .await
            .unwrap();

    let start_resp = client
        .start(Request::new(StartRequest {
            token: token.clone(),
        }))
        .await
        .unwrap();
    assert!(start_resp.into_inner().success);
    assert!(running.load(std::sync::atomic::Ordering::SeqCst));

    let state_resp = client
        .get_state(Request::new(GetStateRequest {
            token: token.clone(),
        }))
        .await
        .unwrap();
    let state: BackendTradingState =
        serde_json::from_str(&state_resp.into_inner().json_data).unwrap();
    assert_eq!(state.price, 123.45);
    assert_eq!(state.timestamp_ms, Some(1600000000000));
    assert_eq!(state.history_points.len(), 2);

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn test_reconnect_logic() {
    let addr: SocketAddr = "[::1]:50054".parse().unwrap();
    let token = "test-token".to_string();

    let (tx_close1, rx_close1) = oneshot::channel::<()>();
    let engine1 = MyDataEngine {
        token: token.clone(),
        ..Default::default()
    };
    let server_handle1 = tokio::spawn(async move {
        Server::builder()
            .add_service(DataEngineServer::new(engine1))
            .serve_with_shutdown(addr, async {
                rx_close1.await.ok();
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let url = format!("http://{}", addr);
    let mut client =
        backcast::trading::engine::data_engine_client::DataEngineClient::connect(url.clone())
            .await
            .unwrap();

    let _ = tx_close1.send(());
    server_handle1.await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let err = client
        .get_state(Request::new(GetStateRequest {
            token: token.clone(),
        }))
        .await
        .unwrap_err();
    assert!(err.code() == tonic::Code::Unavailable || err.code() == tonic::Code::Internal);

    let (tx_close2, rx_close2) = oneshot::channel::<()>();
    let engine2 = MyDataEngine {
        token: token.clone(),
        ..Default::default()
    };
    let server_handle2 = tokio::spawn(async move {
        Server::builder()
            .add_service(DataEngineServer::new(engine2))
            .serve_with_shutdown(addr, async {
                rx_close2.await.ok();
            })
            .await
            .unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    if let Ok(new_client) =
        backcast::trading::engine::data_engine_client::DataEngineClient::connect(url.clone()).await
    {
        client = new_client;
    }

    let state_resp = client
        .get_state(Request::new(GetStateRequest {
            token: token.clone(),
        }))
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<BackendTradingState>(&state_resp.into_inner().json_data)
            .unwrap()
            .price,
        123.45
    );

    let _ = tx_close2.send(());
    server_handle2.await.unwrap();
}

/// Health servicer that returns NOT_SERVING for the first `n` Check calls, then
/// SERVING. Used by the delayed-SERVING-within-budget attach test.
struct DelayedHealth {
    not_serving_count: usize,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[tonic::async_trait]
impl Health for DelayedHealth {
    async fn check(
        &self,
        _req: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let status = if n < self.not_serving_count {
            ServingStatus::NotServing as i32
        } else {
            ServingStatus::Serving as i32
        };
        Ok(Response::new(HealthCheckResponse { status }))
    }
}

/// Health servicer whose status is read from a shared atomic, so a test can
/// flip it mid-run (SERVING -> NOT_SERVING / unavailable) to exercise the
/// post-Ready monitor (Step 5-1).
struct SwitchableHealth {
    status: std::sync::Arc<std::sync::atomic::AtomicI32>,
    /// When true, Check returns Err(unavailable) instead of the status code,
    /// to simulate a hard crash (connection-level failure).
    unavailable: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[tonic::async_trait]
impl Health for SwitchableHealth {
    async fn check(
        &self,
        _req: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(Status::unavailable("backend down"));
        }
        Ok(Response::new(HealthCheckResponse {
            status: self.status.load(std::sync::atomic::Ordering::SeqCst),
        }))
    }
}

/// Drive `run_supervisor` against an already-listening attach server and wait
/// for a terminal lifecycle state (Ready or StartupFailed), 5s budget.
async fn run_attach_and_await_terminal(config: SupervisorConfig) -> BackendLifecycle {
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (ct, cr) = mpsc::unbounded_channel();
    drop(ct);
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
    .expect("supervisor reached a terminal state within 5s")
    .expect("watch channel stayed open");
    *lr.borrow()
}

#[tokio::test]
async fn attach_serving_then_getstate_ok_reaches_ready() {
    let addr: SocketAddr = "127.0.0.1:50061".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let health = MockHealth {
        status: ServingStatus::Serving as i32,
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let outcome = run_attach_and_await_terminal(config).await;
    assert_eq!(outcome, BackendLifecycle::Ready);

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn attach_service_unknown_reaches_identity_mismatch() {
    let addr: SocketAddr = "127.0.0.1:50062".parse().unwrap();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let health = MockHealth {
        status: ServingStatus::ServiceUnknown as i32,
    };
    let engine = MyDataEngine {
        token: "good-token".to_string(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: "good-token".to_string(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let outcome = run_attach_and_await_terminal(config).await;
    assert_eq!(
        outcome,
        BackendLifecycle::StartupFailed("BACKEND_IDENTITY_MISMATCH")
    );

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn attach_getstate_unauthenticated_reaches_token_mismatch() {
    let addr: SocketAddr = "127.0.0.1:50063".parse().unwrap();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let health = MockHealth {
        status: ServingStatus::Serving as i32,
    };
    // Server token differs from the supervisor config token -> get_state 401.
    let engine = MyDataEngine {
        token: "server-token".to_string(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: "client-token".to_string(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let outcome = run_attach_and_await_terminal(config).await;
    assert_eq!(
        outcome,
        BackendLifecycle::StartupFailed("BACKEND_TOKEN_MISMATCH")
    );

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn attach_delayed_serving_within_budget_reaches_ready() {
    let addr: SocketAddr = "127.0.0.1:50064".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let health = DelayedHealth {
        not_serving_count: 3,
        calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let outcome = run_attach_and_await_terminal(config).await;
    assert_eq!(outcome, BackendLifecycle::Ready);

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn post_ready_health_failures_reach_crashed() {
    let addr: SocketAddr = "127.0.0.1:50071".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let status = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(
        ServingStatus::Serving as i32,
    ));
    let unavailable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let health = SwitchableHealth {
        status: status.clone(),
        unavailable: unavailable.clone(),
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (_ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));

    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Ready)),
    )
    .await
    .expect("reached Ready")
    .expect("watch open");

    unavailable.store(true, std::sync::atomic::Ordering::SeqCst);

    tokio::time::timeout(
        Duration::from_secs(3),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Crashed)),
    )
    .await
    .expect("reached Crashed")
    .expect("watch open");

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn post_ready_not_serving_reaches_stopped() {
    let addr: SocketAddr = "127.0.0.1:50072".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let status = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(
        ServingStatus::Serving as i32,
    ));
    let unavailable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let health = SwitchableHealth {
        status: status.clone(),
        unavailable: unavailable.clone(),
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (_ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));

    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Ready)),
    )
    .await
    .expect("reached Ready")
    .expect("watch open");

    status.store(
        ServingStatus::NotServing as i32,
        std::sync::atomic::Ordering::SeqCst,
    );

    tokio::time::timeout(
        Duration::from_secs(3),
        lr.wait_for(|s| matches!(s, BackendLifecycle::ShuttingDown)),
    )
    .await
    .expect("reached ShuttingDown")
    .expect("watch open");

    tokio::time::timeout(
        Duration::from_secs(10),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Stopped)),
    )
    .await
    .expect("reached Stopped")
    .expect("watch open");

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn post_ready_not_serving_recovers_to_ready() {
    let addr: SocketAddr = "127.0.0.1:50073".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let status = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(
        ServingStatus::Serving as i32,
    ));
    let unavailable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let health = SwitchableHealth {
        status: status.clone(),
        unavailable: unavailable.clone(),
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (_ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));

    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Ready)),
    )
    .await
    .expect("reached Ready")
    .expect("watch open");

    status.store(
        ServingStatus::NotServing as i32,
        std::sync::atomic::Ordering::SeqCst,
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        lr.wait_for(|s| matches!(s, BackendLifecycle::ShuttingDown)),
    )
    .await
    .expect("reached ShuttingDown")
    .expect("watch open");

    status.store(
        ServingStatus::Serving as i32,
        std::sync::atomic::Ordering::SeqCst,
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Ready)),
    )
    .await
    .expect("recovered to Ready")
    .expect("watch open");

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}

#[tokio::test]
async fn shutdown_command_attach_reaches_stopped() {
    let addr: SocketAddr = "127.0.0.1:50074".parse().unwrap();
    let token = "good-token".to_string();
    let (tx_close, rx_close) = oneshot::channel::<()>();

    let status = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(
        ServingStatus::Serving as i32,
    ));
    let unavailable = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let health = SwitchableHealth {
        status: status.clone(),
        unavailable: unavailable.clone(),
    };
    let engine = MyDataEngine {
        token: token.clone(),
        ..Default::default()
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
    tokio::time::sleep(Duration::from_millis(100)).await;

    let config = SupervisorConfig {
        enabled: true,
        url: format!("http://{}", addr),
        token: token.clone(),
        autospawn: false,
        cwd: None,
        python_bin: None,
    };
    let (lt, mut lr) = watch::channel(BackendLifecycle::Disabled);
    let (ct, cr) = mpsc::unbounded_channel();
    let (ownership_tx, _ownership_rx) =
        watch::channel(backcast::backend_supervisor::BackendOwnership::default());
    tokio::spawn(run_supervisor(config, lt, cr, ownership_tx));

    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Ready)),
    )
    .await
    .expect("reached Ready")
    .expect("watch open");

    ct.send(SupervisorCommand::Shutdown {
        grace_seconds: 0,
        reply_tx: None,
    })
    .expect("send Shutdown");

    tokio::time::timeout(
        Duration::from_secs(5),
        lr.wait_for(|s| matches!(s, BackendLifecycle::Stopped)),
    )
    .await
    .expect("reached Stopped")
    .expect("watch open");

    let _ = tx_close.send(());
    server_handle.await.unwrap();
}
