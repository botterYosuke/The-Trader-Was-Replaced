//! P5（kind:arch）InProcTransport — live 系コマンドが InprocLiveServer 経由で
//! GrpcDataEngineServer に直接ルーティングされること
//!
//! # 未実装（doc stub）
//!
//! `BACKEND_TRANSPORT=inproc` + `LIVE_VENUE=MOCK` で
//! `VenueLogin → SetExecutionMode(LiveManual) → VenueLogout` のコマンド列が
//! gRPC なしで inproc Python call に到達すること。
//!
//! `BackendEvent`（`VenueLogoutDetected` 等）が
//! `publish_backend_event` → `_rust_event_sink.push()` 経由で
//! Rust チャネルに到達することも確認する。
//!
//! 実 Python インタープリタを要するため `INPROC_E2E=1` ゲート。
//!
//! # Python 側カバレッジ（既実装）
//!
//! `python/tests/test_inproc_e2e.py` の以下テストが Python 経路を検証済み（`INPROC_E2E=1` gate）:
//! - `test_p5_close_calls_teardown_and_stop_live_loop` — close/teardown 契約
//! - `test_p5_venue_login_venue_logout_cycle_no_adapter` — VenueLogin → VenueLogout cycling
//! - `test_p5_list_instruments_local_routing` — ListInstruments routing
//! - `test_p5_set_execution_mode_live_manual_no_adapter` — SetExecutionMode(LiveManual) routing
//!
//! Python unit tests（`python/tests/test_inproc_server.py`）が単体で全 20 メソッドの
//! 例外捕捉（INPROC_ABORT / INPROC_ERROR）および teardown 契約を GREEN で保護。
//!
//! # 残作業（Rust 統合テスト）
//!
//! Rust 側の `inproc_dispatch` が各 `TransportCommand` を対応する
//! `InprocLiveServer` メソッドへ正しくルーティングすること、および
//! `RustEventSink.push(bytes)` → `BackendEvent` decode → `mpsc::UnboundedSender` 送出
//! の全行程を実 PyO3 ランタイムで確認する統合テストが未実装。
//!
//! ```ignore
//! #[test]
//! #[cfg_attr(not(env = "INPROC_E2E"), ignore)]
//! fn p5_inproc_live_routing_no_adapter() {
//!     // start InProcTransport with live_venue_id="MOCK"
//!     // send VenueLogin → assert StatusUpdate sequence (LIVE_ADAPTER_NOT_CONFIGURED)
//!     // send SetExecutionMode(LiveManual) → assert echo
//!     // send VenueLogout → assert success
//!     // verify no BackendEvent leaks
//! }
//! ```
//!
//! issue #64 Phase 4 / フォロータスク③ 参照。
