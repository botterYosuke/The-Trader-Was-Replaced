use backcast::trading::engine::{
    EngineState, ForceStopReplayRequest, GetPortfolioRequest, GetPortfolioResponse,
    GetStateRequest, GetStateResponse, ListInstrumentsRequest, ListInstrumentsResponse,
    LoadReplayDataRequest, PauseReplayRequest, ReplayControlResponse, ResumeReplayRequest,
    SetReplaySpeedRequest, StartEngineRequest, StartEngineResponse, StartResponse,
    StepReplayRequest, StopEngineRequest, StopReplayRequest, StopRequest, StopResponse,
    data_engine_server::{DataEngine, DataEngineServer},
};
use backcast::trading::{BackendTradingState, StartRequest};
use serde_json::json;
use std::net::SocketAddr;
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, transport::Server};

// Mock gRPC server implementation
#[derive(Default)]
pub struct MyDataEngine {
    pub token: String,
    pub running: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
