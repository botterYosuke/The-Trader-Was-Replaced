//! Shared state for the replay startup progress UI: resource + phase enum.
//! Marker components and system wiring live in `ui::run_result_panel`.

use bevy::prelude::Resource;

#[derive(Resource, Default, Debug, Clone)]
pub struct ReplayStartupProgress {
    pub visible: bool,
    pub phase: ReplayStartupPhase,
    pub detail: Option<String>,
    pub error: Option<String>,
    /// `Time<Real>::elapsed()` 基準の起動時刻。
    pub started_at_elapsed: Option<std::time::Duration>,
    /// Run 押下時点で観測されていた `TradingSession.timestamp_ms`。
    pub baseline_timestamp_ms: Option<i64>,
    /// UI が Run ごとに採番する startup id。
    pub startup_id: u64,
    pub next_startup_id: u64,
    /// matching startup の StartEngine accepted gate。
    pub start_engine_accepted: bool,
}

/// `Failed` は意図的に含めない —— `error.is_some()` で表す。
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayStartupPhase {
    #[default]
    Idle,
    CommandAccepted,
    ResettingReplay,
    LoadingData,
    StartingStrategy,
    WaitingForFirstTick,
}
