//! N13 footer_live_auto_pause_resume — Live Auto 実行中に footer ▶ が
//! `PauseLiveStrategy` / `ResumeLiveStrategy` を送り、ラベルが `||` / `▶` に変わること。
//!
//! issue #42 Slice 4 の受け入れ基準:
//! - RUNNING: ▶ → `||` 表示 + `PauseLiveStrategy{run_id}` 送出
//! - PAUSED:  `||` → `▶` 表示 + `ResumeLiveStrategy{run_id}` 送出
//! - 実行中でない: `▶` 表示 + `StartLiveAuto` 送出 (N5/N6 でカバー)
//! 詳細は `tests/e2e/FLOWS.md` の N13 を参照。

use serial_test::serial;

use bevy::prelude::*;

use backcast::trading::{
    BackendStatus, CurrentRun, ExecutionMode, ExecutionModeRes, ReplaySpeed, RunState,
    SelectedSymbol, TradingSession, TradingSettings, TransportCommand, TransportCommandSender,
    VenueState, VenueStatusRes,
};
use backcast::ui::components::{
    PauseResumeLabel, ScenarioMetadata, StrategyBuffer, StrategyRunRequested,
};
use backcast::ui::footer::{
    apply_execution_mode_visibility_system, footer_pause_resume_system, spawn_footer,
    update_footer_system,
};
use backcast::ui::strategy_editor::StrategyAutoSaveState;

use tokio::sync::mpsc;

use crate::support::Harness;

// ── Command dispatch tests (Harness) ────────────────────────────────────────

#[test]
fn n13_running_pause_sends_pause_live_strategy() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);

    // Seed an active running run
    {
        let mut cr = h.app.world_mut().resource_mut::<CurrentRun>();
        cr.run_id = Some("r-live".to_string());
        cr.state = RunState::Running;
    }

    h.click_pause_resume();

    let cmds = h.drain_commands();
    assert!(
        cmds.iter()
            .any(|cmd| matches!(cmd, TransportCommand::PauseLiveStrategy { run_id } if run_id == "r-live")),
        "Running LiveAuto: ▶ press must send PauseLiveStrategy{{run_id:'r-live'}}. Got: {cmds:?}",
    );
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, TransportCommand::StartLiveAuto { .. })),
        "Running LiveAuto: ▶ press must NOT send StartLiveAuto. Got: {cmds:?}",
    );
}

#[test]
fn n13_paused_resume_sends_resume_live_strategy() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);

    {
        let mut cr = h.app.world_mut().resource_mut::<CurrentRun>();
        cr.run_id = Some("r-live".to_string());
        cr.state = RunState::Paused;
    }

    h.click_pause_resume();

    let cmds = h.drain_commands();
    assert!(
        cmds.iter().any(|cmd| matches!(
            cmd,
            TransportCommand::ResumeLiveStrategy { run_id } if run_id == "r-live"
        )),
        "Paused LiveAuto: ▶ press must send ResumeLiveStrategy{{run_id:'r-live'}}. Got: {cmds:?}",
    );
    assert!(
        !cmds
            .iter()
            .any(|cmd| matches!(cmd, TransportCommand::StartLiveAuto { .. })),
        "Paused LiveAuto: ▶ press must NOT send StartLiveAuto. Got: {cmds:?}",
    );
}

// ── Label update tests (real footer, N6 pattern) ─────────────────────────────

fn make_label_app() -> (App, mpsc::UnboundedReceiver<TransportCommand>) {
    let mut app = App::new();
    let (tx, rx) = mpsc::unbounded_channel::<TransportCommand>();

    app.add_plugins(MinimalPlugins)
        .add_plugins(AssetPlugin::default())
        .init_asset::<Font>();

    app.insert_resource(TransportCommandSender { tx })
        .insert_resource(ExecutionModeRes::default())
        .insert_resource(TradingSession::default())
        .insert_resource(BackendStatus::default())
        .insert_resource(TradingSettings::default())
        .insert_resource(ReplaySpeed::default())
        .insert_resource(StrategyBuffer::default())
        .insert_resource(CurrentRun::default())
        .insert_resource(SelectedSymbol::default())
        .insert_resource(VenueStatusRes::default())
        .insert_resource(ScenarioMetadata::default())
        .insert_resource(StrategyAutoSaveState::default())
        .add_message::<StrategyRunRequested>();
    app.add_plugins(backcast::ui::theme::ThemePlugin);

    app.add_systems(Startup, spawn_footer);
    app.add_systems(
        Update,
        (
            apply_execution_mode_visibility_system,
            footer_pause_resume_system,
            update_footer_system,
        ),
    );

    (app, rx)
}

fn pause_resume_label_text(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<PauseResumeLabel>>()
        .iter(app.world())
        .next()
        .map(|t| t.0.clone())
        .unwrap_or_default()
}

#[test]
#[serial]
fn n13_running_label_is_pause_icon() {
    let (mut app, _rx) = make_label_app();
    app.update(); // Startup: spawn footer

    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.run_id = Some("r-live".to_string());
        cr.state = RunState::Running;
    }
    app.update();

    assert_eq!(
        pause_resume_label_text(&mut app),
        "||",
        "LiveAuto RUNNING → label must be '||' (pause icon)"
    );
}

#[test]
#[serial]
fn n13_paused_label_is_play_icon() {
    let (mut app, _rx) = make_label_app();
    app.update();

    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    {
        let mut cr = app.world_mut().resource_mut::<CurrentRun>();
        cr.run_id = Some("r-live".to_string());
        cr.state = RunState::Paused;
    }
    app.update();

    assert_eq!(
        pause_resume_label_text(&mut app),
        "▶",
        "LiveAuto PAUSED → label must be '▶' (play/resume icon)"
    );
}

#[test]
#[serial]
fn n13_idle_label_is_play_icon() {
    let (mut app, _rx) = make_label_app();
    app.update();

    app.world_mut().resource_mut::<ExecutionModeRes>().mode = ExecutionMode::LiveAuto;
    // CurrentRun.state = Idle (default), run_id = None
    app.update();

    assert_eq!(
        pause_resume_label_text(&mut app),
        "▶",
        "LiveAuto Idle → label must be '▶'"
    );
}

// ── Double-press guard (M2 / issue #42 review) ───────────────────────────────

/// LiveAuto の footer ▶ を、まだ run が立ち上がる前（state Idle / run_id None）に
/// 2 回続けて押しても `StartLiveAuto` は 1 件しか出てはならない。実マネー戦略の
/// 二重起動を防ぐガード。Replay 経路は `RunState::Running` で既にガード済みだが
/// (footer.rs:624 付近)、LiveAuto 経路は非対称で StartLiveAuto 後に state を倒さず
/// run_id も無いため、2 回目の押下が同じ pre-flight を素通りして 2 通目を送る。
///
/// 全 pre-flight (exec mode / scenario / venue / cache) を満たすので 1 回目は
/// 確実に StartLiveAuto を出す。`click_pause_resume` は毎回新 entity を spawn し
/// Changed<Interaction> を満たすので、2 回呼べば handler は 2 回 fire する。
/// fix 前は starts==2 で RED、ガード追加後は starts==1 で GREEN。
#[test]
#[serial]
fn n13_live_auto_double_press_starts_run_only_once() {
    let mut h = Harness::new();
    h.set_exec_mode(ExecutionMode::LiveAuto);
    h.set_scenario_instruments(&["7203.TSE"]);
    h.set_venue(VenueState::Subscribed, "tachibana");
    h.set_strategy_cache_path("strategy_cache.py");
    // CurrentRun は default のまま: run_id None / state Idle（= まだ起動前）。

    h.click_pause_resume(); // 1回目: StartLiveAuto が出る
    h.click_pause_resume(); // 2回目: ガードがあれば出ない（fix 前は 2 通目が出る）

    let cmds = h.drain_commands();
    let starts = cmds
        .iter()
        .filter(|cmd| matches!(cmd, TransportCommand::StartLiveAuto { .. }))
        .count();
    assert_eq!(
        starts, 1,
        "LiveAuto: 起動前の ▶ 二度押しは StartLiveAuto を 1 件だけにすべき（実マネー戦略の二重起動防止）。Got: {cmds:?}",
    );
}
