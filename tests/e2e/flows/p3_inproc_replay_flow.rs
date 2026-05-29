//! P3（kind:arch）InProcTransport — IDLE → LoadAndStep → LOADED → StepForward
//! → Pause → Resume → ForceStop → IDLE
//!
//! # 未実装（doc stub）
//!
//! `BACKEND_TRANSPORT=inproc` で `InProcTransport` が選択され、
//! `SupervisorConfig.inproc=true` で supervisor が即 `BackendLifecycle::Ready` を emit
//! し、Python `DataEngine` の各 replay メソッドが直接 PyO3 経由で呼ばれること（gRPC なし）。
//!
//! `BackendTradingState` JSON round-trip（`model_dump_json` → `serde_json::from_str::<BackendTradingState>`）
//! が壊れないことも確認する。
//!
//! 実 Python インタープリタを要するため `INPROC_E2E=1` ゲート。
//!
//! # Python 側カバレッジ（既実装）
//!
//! `python/tests/test_inproc_e2e.py` の以下テストが Python 経路を検証済み（`INPROC_E2E=1` gate）:
//! - `test_p3_get_state_json_contains_required_fields` — JSON roundtrip
//! - `test_p3_force_stop_replay_is_graceful_from_idle` — ForceStop no-op
//! - `test_p3_set_execution_mode_then_get_state_consistent` — SetExecutionMode routing
//!
//! # 残作業（Rust 統合テスト）
//!
//! Rust 側の `InProcTransport::run` 経路（channel setup → Python worker thread spawn →
//! Connected(true) emit → inproc_dispatch → BackendTradingState push）を
//! 以下の要素で end-to-end 検証する実 PyO3 統合テストが未実装:
//!
//! ```ignore
//! #[test]
//! #[cfg_attr(not(env = "INPROC_E2E"), ignore)]
//! fn p3_inproc_replay_flow() {
//!     use backcast::backend_transport::InProcTransport;
//!     use backcast::trading::{BackendLifecycle, BackendStatusUpdate, TransportCommand};
//!     use tokio::sync::{mpsc, watch};
//!
//!     let rt = tokio::runtime::Runtime::new().unwrap();
//!     let python_engine_path = std::env::var("PYTHON_ENGINE_PATH")
//!         .unwrap_or_else(|_| "python".to_string());
//!
//!     let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<TransportCommand>();
//!     let (state_tx, mut state_rx) = mpsc::unbounded_channel();
//!     let (status_tx, mut status_rx) = mpsc::unbounded_channel();
//!     let (event_tx, _event_rx) = mpsc::unbounded_channel();
//!     let (_lc_tx, lc_rx) = watch::channel(BackendLifecycle::NotStarted);
//!
//!     let transport = InProcTransport {
//!         python_engine_path,
//!         catalog_path: None,
//!         max_history_len: 100,
//!         poll_interval_ms: 200,
//!         live_venue_id: None,
//!     };
//!
//!     let _run_handle = std::thread::spawn(move || {
//!         rt.block_on(transport.run(cmd_rx, state_tx, status_tx, event_tx, lc_rx))
//!     });
//!
//!     // Wait for Connected(true)
//!     // ... assert BackendTradingState JSON arrives
//!     // ... send ForceStop, assert state returns to IDLE
//! }
//! ```
//!
//! issue #64 Phase 2 / フォロータスク③ 参照。
