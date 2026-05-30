//! A18 (kind:unit): IDLE → ▶| (LoadAndStep) 完了後に ▶ (Run) がブロックされないこと。
//!
//! `StepFromIdleRequested` ハンドラが `current_run.state = Running` をセットするが、
//! `LoadAndStep` は `RunComplete` を送らないため、ユーザーが再び ▶ を押しても
//! `"Run blocked: already running"` で弾かれる（issue #71）。
//!
//! seam: `CurrentRun.state = Running` + `TradingSession.replay_state = "LOADED"` の状態で
//!       `PauseResumeButton` を Interaction::Pressed → `footer_pause_resume_system` が
//!       `RunState::Running` ガードを通過させるか確認。
//!
//! 実装: `src/ui/footer.rs #[cfg(test)] a18_run_not_blocked_when_backend_loaded_despite_running_state`
//! を `cargo test --lib footer::tests::a18` で実行。
//!
//! fix: `menu_bar.rs` の `StepFromIdleRequested` ハンドラで `current_run.state = RunState::Running`
//!      をセットしない（`LoadAndStep` は full run ではなく、`RunComplete` が来ないため永続ロックする）。
