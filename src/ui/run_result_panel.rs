use crate::replay::{ReplayStartupPhase, ReplayStartupProgress};
use crate::trading::{CurrentRun, RunState};
use crate::ui::components::{PanelKind, RunResultPanelRoot};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

// ── レイアウト & 配色 ─────────────────────────────────────────
const PANEL_SIZE: Vec2 = Vec2::new(280.0, 160.0);
const PANEL_POSITION: Vec2 = Vec2::new(-450.0, -70.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4);

const COLOR_DEFAULT: Color = Color::srgb(0.85, 0.88, 0.94);
const COLOR_IDLE: Color = Color::srgb(0.55, 0.55, 0.55);
const COLOR_RUNNING: Color = Color::srgb(1.0, 0.78, 0.0);
const COLOR_COMPLETED: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_FAILED: Color = Color::srgb(1.0, 0.20, 0.40);
const COLOR_RUNID: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_PNL_POS: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_PNL_NEG: Color = Color::srgb(1.0, 0.20, 0.40);

const BAR_WIDTH: f32 = 230.0;
const BAR_HEIGHT: f32 = 8.0;
const BAR_FILL_WIDTH: f32 = BAR_WIDTH * 0.30;

// ── 通常行マーカー ───────────────────────────────────────────
#[derive(Component, Clone, Copy)]
pub enum RunResultLabel {
    State,
    RunId,
    Stats,
    Pnl,
}

// ── 起動進捗セクションマーカー ───────────────────────────────
#[derive(Component)]
pub struct RunResultPhaseLabel;

#[derive(Component)]
pub struct RunResultBarBg;

#[derive(Component)]
pub struct RunResultBarFill;

// ── Spawn ────────────────────────────────────────────────────
pub fn spawn_run_result_panel(commands: &mut Commands) {
    let (root, content_area, _title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "RUN RESULT".to_string(),
            size: PANEL_SIZE,
            position: PANEL_POSITION,
            accent: ACCENT,
            closeable: false,
            resizable: false,
        },
    );
    commands.entity(root).insert((PanelKind::RunResult, RunResultPanelRoot));

    spawn_row(commands, content_area, RunResultLabel::State, 33.0);
    spawn_row(commands, content_area, RunResultLabel::RunId, 11.0);
    spawn_row(commands, content_area, RunResultLabel::Stats, -11.0);
    spawn_row(commands, content_area, RunResultLabel::Pnl, -33.0);

    let phase_label = commands
        .spawn((
            Text2d::new(""),
            TextFont { font_size: 12.0, ..default() },
            TextColor(COLOR_DEFAULT),
            Transform::from_xyz(0.0, 10.0, 0.1),
            Visibility::Hidden,
            RunResultPhaseLabel,
        ))
        .id();
    commands.entity(content_area).add_child(phase_label);

    let bar_bg = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.15, 0.15, 0.22, 1.0),
                custom_size: Some(Vec2::new(BAR_WIDTH, BAR_HEIGHT)),
                ..default()
            },
            Transform::from_xyz(0.0, -10.0, 0.1),
            Visibility::Hidden,
            RunResultBarBg,
        ))
        .id();
    commands.entity(content_area).add_child(bar_bg);

    let bar_fill = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.35, 0.55, 0.85, 1.0),
                custom_size: Some(Vec2::new(BAR_FILL_WIDTH, BAR_HEIGHT)),
                ..default()
            },
            Transform::from_xyz(-(BAR_WIDTH - BAR_FILL_WIDTH) / 2.0, 0.0, 0.05),
            RunResultBarFill,
        ))
        .id();
    commands.entity(bar_bg).add_child(bar_fill);
}

pub fn spawn_run_result_panel_system(mut commands: Commands) {
    spawn_run_result_panel(&mut commands);
}

fn spawn_row(commands: &mut Commands, parent: Entity, kind: RunResultLabel, y: f32) {
    let entity = commands
        .spawn((
            Text2d::new(""),
            TextFont { font_size: 12.0, ..default() },
            TextColor(COLOR_DEFAULT),
            Transform::from_xyz(0.0, y, 0.1),
            kind,
        ))
        .id();
    commands.entity(parent).add_child(entity);
}

pub fn run_result_panel_system(
    current_run: Res<CurrentRun>,
    progress: Res<ReplayStartupProgress>,
    mut label_q: Query<(&RunResultLabel, &mut Text2d, &mut TextColor)>,
    mut phase_q: Query<
        (&mut Text2d, &mut Visibility),
        (With<RunResultPhaseLabel>, Without<RunResultLabel>),
    >,
    mut bar_bg_q: Query<
        &mut Visibility,
        (With<RunResultBarBg>, Without<RunResultPhaseLabel>, Without<RunResultLabel>),
    >,
) {
    let startup_active = progress.visible && progress.error.is_none();

    if let Ok((mut phase_text, mut phase_vis)) = phase_q.single_mut() {
        let target_vis = if startup_active { Visibility::Inherited } else { Visibility::Hidden };
        if *phase_vis != target_vis {
            *phase_vis = target_vis;
        }
        let label = if startup_active { phase_label_text(progress.phase) } else { "" };
        if phase_text.0 != label {
            phase_text.0 = label.to_string();
        }
    }

    if let Ok(mut bar_vis) = bar_bg_q.single_mut() {
        let target = if startup_active { Visibility::Inherited } else { Visibility::Hidden };
        if *bar_vis != target {
            *bar_vis = target;
        }
    }

    for (kind, mut text, mut color) in &mut label_q {
        let (new_text, new_color) = if startup_active {
            (String::new(), COLOR_DEFAULT)
        } else {
            normal_row_content(kind, &current_run)
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
        if color.0 != new_color {
            color.0 = new_color;
        }
    }
}

fn phase_label_text(phase: ReplayStartupPhase) -> &'static str {
    match phase {
        ReplayStartupPhase::Idle => "",
        ReplayStartupPhase::CommandAccepted => "Starting replay command...",
        ReplayStartupPhase::ResettingReplay => "Resetting previous replay...",
        ReplayStartupPhase::LoadingData => "Loading replay data...",
        ReplayStartupPhase::StartingStrategy => "Starting Python strategy...",
        ReplayStartupPhase::WaitingForFirstTick => "Waiting for first replay tick...",
    }
}

fn normal_row_content(kind: &RunResultLabel, current_run: &CurrentRun) -> (String, Color) {
    match kind {
        RunResultLabel::State => match &current_run.state {
            RunState::Idle => ("No run yet".to_string(), COLOR_IDLE),
            RunState::Running => ("Running…".to_string(), COLOR_RUNNING),
            RunState::Paused => ("Paused".to_string(), COLOR_RUNNING),
            RunState::Stopped => ("Stopped".to_string(), COLOR_IDLE),
            RunState::Completed => ("Completed".to_string(), COLOR_COMPLETED),
            RunState::Failed { error } => (format!("Failed: {}", error), COLOR_FAILED),
        },
        RunResultLabel::RunId => match &current_run.run_id {
            Some(id) => (format!("run: {}", id), COLOR_RUNID),
            None => (String::new(), COLOR_DEFAULT),
        },
        RunResultLabel::Stats => match &current_run.state {
            RunState::Running | RunState::Paused if !current_run.strategy_name.is_empty() => (
                format!(
                    "strat: {}  o:{} f:{}",
                    current_run.strategy_name, current_run.order_count, current_run.fill_count
                ),
                COLOR_DEFAULT,
            ),
            RunState::Running | RunState::Paused => (String::new(), COLOR_DEFAULT),
            _ => match &current_run.parsed_summary {
                Some(s) => (
                    format!("fills:{}  sh:{:.2}  dd:{:.0}", s.fills_count, s.sharpe, s.max_drawdown),
                    COLOR_DEFAULT,
                ),
                None if current_run.order_count > 0 || current_run.fill_count > 0 => (
                    format!(
                        "strat: {}  o:{} f:{}",
                        current_run.strategy_name,
                        current_run.order_count,
                        current_run.fill_count
                    ),
                    COLOR_DEFAULT,
                ),
                None => (String::new(), COLOR_DEFAULT),
            },
        },
        RunResultLabel::Pnl => match &current_run.state {
            RunState::Running | RunState::Paused if !current_run.strategy_name.is_empty() => {
                let c = if current_run.realized_pnl + current_run.unrealized_pnl >= 0.0 {
                    COLOR_PNL_POS
                } else {
                    COLOR_PNL_NEG
                };
                (
                    format!(
                        "pnl: {:.0} / unrlz: {:.0}",
                        current_run.realized_pnl, current_run.unrealized_pnl
                    ),
                    c,
                )
            }
            RunState::Running | RunState::Paused => (String::new(), COLOR_DEFAULT),
            _ => match &current_run.parsed_summary {
                Some(s) => {
                    let c = if s.total_pnl >= 0.0 { COLOR_PNL_POS } else { COLOR_PNL_NEG };
                    (format!("pnl:{:.0}  so:{:.2}", s.total_pnl, s.sortino), c)
                }
                None
                    if current_run.realized_pnl != 0.0
                        || current_run.unrealized_pnl != 0.0 =>
                {
                    let c = if current_run.realized_pnl + current_run.unrealized_pnl >= 0.0 {
                        COLOR_PNL_POS
                    } else {
                        COLOR_PNL_NEG
                    };
                    (
                        format!(
                            "pnl: {:.0} / unrlz: {:.0}",
                            current_run.realized_pnl, current_run.unrealized_pnl
                        ),
                        c,
                    )
                }
                None => (String::new(), COLOR_DEFAULT),
            },
        },
    }
}

/// 三角波でバーフィルを往復アニメーションさせる。
pub fn animate_run_result_startup_bar_system(
    time: Res<Time<Real>>,
    progress: Res<ReplayStartupProgress>,
    mut fill_q: Query<&mut Transform, With<RunResultBarFill>>,
) {
    if !progress.visible || progress.error.is_some() {
        return;
    }
    let t = time.elapsed_secs() % 2.0;
    let phase = if t < 1.0 { t } else { 2.0 - t };
    let travel = BAR_WIDTH - BAR_FILL_WIDTH;
    let x = -(travel / 2.0) + travel * phase;
    if let Ok(mut tf) = fill_q.single_mut() {
        tf.translation.x = x;
    }
}

/// 経路 B: TradingSession の replay_state が RUNNING になる、もしくは timestamp が動いたら hide。
pub fn auto_hide_startup_progress_system(
    mut progress: ResMut<ReplayStartupProgress>,
    trading: Res<crate::trading::TradingSession>,
) {
    if !progress.visible || progress.error.is_some() || !progress.start_engine_accepted {
        return;
    }
    let running = trading.replay_state.as_deref() == Some("RUNNING");
    let timestamp_changed = matches!(
        progress.baseline_timestamp_ms,
        Some(b) if trading.timestamp_ms != b
    );
    if running || timestamp_changed {
        progress.visible = false;
        progress.phase = ReplayStartupPhase::Idle;
        progress.detail = None;
        progress.baseline_timestamp_ms = None;
        progress.started_at_elapsed = None;
        progress.start_engine_accepted = false;
    }
}

/// 60s 経っても startup が完了しなければ soft timeout として error をセットする。
pub fn replay_startup_timeout_system(
    time: Res<Time<Real>>,
    mut progress: ResMut<ReplayStartupProgress>,
) {
    if !progress.visible || progress.error.is_some() {
        return;
    }
    let Some(started) = progress.started_at_elapsed else {
        return;
    };
    let elapsed = time.elapsed().saturating_sub(started);
    if elapsed >= std::time::Duration::from_secs(60) {
        progress.error = Some(
            "Replay startup is taking longer than expected. Check backend logs or try Force Stop."
                .to_string(),
        );
    }
}

pub fn apply_run_result_visibility_system(
    exec_mode: Res<crate::trading::ExecutionModeRes>,
    mut panel_q: Query<&mut Visibility, With<RunResultPanelRoot>>,
) {
    let target = match exec_mode.mode {
        crate::trading::ExecutionMode::LiveManual => Visibility::Hidden,
        _ => Visibility::Inherited,
    };
    for mut vis in &mut panel_q {
        if *vis != target {
            *vis = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_hide_by_running_state() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_startup_progress_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = Some("RUNNING".to_string());
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
        assert!(matches!(progress.phase, ReplayStartupPhase::Idle));
    }

    #[test]
    fn old_running_does_not_auto_hide() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_startup_progress_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = false;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = Some("RUNNING".to_string());
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.visible);
    }

    #[test]
    fn auto_hide_by_timestamp_change() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_startup_progress_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.baseline_timestamp_ms = Some(1000);
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = None;
            trading.timestamp_ms = 2000;
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
    }

    #[test]
    fn auto_hide_by_timestamp_regression() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.init_resource::<crate::trading::TradingSession>();
        app.add_systems(Update, auto_hide_startup_progress_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.start_engine_accepted = true;
            progress.baseline_timestamp_ms = Some(2_000_000_000);
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
        }
        {
            let mut trading = app
                .world_mut()
                .resource_mut::<crate::trading::TradingSession>();
            trading.replay_state = None;
            trading.timestamp_ms = 1_000_000_000;
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(!progress.visible);
    }

    #[test]
    fn timeout_sets_error_after_60s() {
        use std::time::Duration;

        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        app.init_resource::<ReplayStartupProgress>();
        app.add_systems(Update, replay_startup_timeout_system);

        {
            let mut progress = app.world_mut().resource_mut::<ReplayStartupProgress>();
            progress.visible = true;
            progress.error = None;
            progress.phase = ReplayStartupPhase::WaitingForFirstTick;
            progress.started_at_elapsed = Some(Duration::ZERO);
        }
        {
            let mut time = app.world_mut().resource_mut::<Time<Real>>();
            time.advance_by(Duration::from_secs(61));
        }

        app.update();

        let progress = app.world().resource::<ReplayStartupProgress>();
        assert!(progress.error.is_some());
        assert!(matches!(
            progress.phase,
            ReplayStartupPhase::WaitingForFirstTick
        ));
    }
}
