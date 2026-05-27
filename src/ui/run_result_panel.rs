use crate::trading::{CurrentRun, RunState};
use crate::ui::components::{PanelKind, RunResultPanelRoot};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};
use bevy::prelude::*;

// ── レイアウト & 配色 ─────────────────────────────────────────
const PANEL_SIZE: Vec2 = Vec2::new(280.0, 160.0);
const PANEL_POSITION: Vec2 = Vec2::new(-450.0, -70.0);
const ACCENT: Color = Color::srgba(0.0, 0.8, 1.0, 0.4); // cyan rim

const COLOR_DEFAULT: Color = Color::srgb(0.85, 0.88, 0.94);
const COLOR_IDLE: Color = Color::srgb(0.55, 0.55, 0.55);
const COLOR_RUNNING: Color = Color::srgb(1.0, 0.78, 0.0);
const COLOR_COMPLETED: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_FAILED: Color = Color::srgb(1.0, 0.20, 0.40);
const COLOR_RUNID: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_PNL_POS: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_PNL_NEG: Color = Color::srgb(1.0, 0.20, 0.40);

// ── 行マーカー ───────────────────────────────────────────────
/// 4 行それぞれを識別するためのマーカー。
#[derive(Component, Clone, Copy)]
pub enum RunResultLabel {
    State,
    RunId,
    Stats,
    Pnl,
}

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

    // 4 行を上から下へ 22px 間隔で配置
    spawn_row(commands, content_area, RunResultLabel::State, 33.0);
    spawn_row(commands, content_area, RunResultLabel::RunId, 11.0);
    spawn_row(commands, content_area, RunResultLabel::Stats, -11.0);
    spawn_row(commands, content_area, RunResultLabel::Pnl, -33.0);
}

pub fn spawn_run_result_panel_system(mut commands: Commands) {
    spawn_run_result_panel(&mut commands);
}

fn spawn_row(commands: &mut Commands, parent: Entity, kind: RunResultLabel, y: f32) {
    let entity = commands
        .spawn((
            Text2d::new(""),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(COLOR_DEFAULT),
            Transform::from_xyz(0.0, y, 0.1),
            kind,
        ))
        .id();
    commands.entity(parent).add_child(entity);
}

/// CurrentRun の現在値を 4 行のテキストに反映する。
/// Replay と LiveAuto の両モードをカバーし、Running/Paused 中は live フィールドを表示する。
pub fn run_result_panel_system(
    current_run: Res<CurrentRun>,
    mut q: Query<(&RunResultLabel, &mut Text2d, &mut TextColor)>,
) {
    for (kind, mut text, mut color) in &mut q {
        let (new_text, new_color) = match kind {
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
                RunState::Running | RunState::Paused => (
                    format!(
                        "strat: {}  o:{} f:{}",
                        current_run.strategy_name, current_run.order_count, current_run.fill_count
                    ),
                    COLOR_DEFAULT,
                ),
                _ => match &current_run.parsed_summary {
                    Some(s) => (
                        format!("fills: {}  eq_pts: {}", s.fills_count, s.equity_points),
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
                RunState::Running | RunState::Paused => {
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
                _ => match &current_run.parsed_summary {
                    Some(s) => {
                        let c = if s.total_pnl >= 0.0 {
                            COLOR_PNL_POS
                        } else {
                            COLOR_PNL_NEG
                        };
                        (format!("pnl: {:.0}", s.total_pnl), c)
                    }
                    None
                        if current_run.realized_pnl != 0.0
                            || current_run.unrealized_pnl != 0.0 =>
                    {
                        let c =
                            if current_run.realized_pnl + current_run.unrealized_pnl >= 0.0 {
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
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
        if color.0 != new_color {
            color.0 = new_color;
        }
    }
}

/// RunResult パネルの可視性を ExecutionMode に合わせて毎フレーム diff-write する。
/// Replay / LiveAuto で可視、LiveManual で非表示（issue #41 仕様）。
/// `is_changed()` ゲートは張らない: mode 変化のないフレームに spawn されたウィンドウも
/// 正しい可視性に補正するため（apply_strategy_editor_mode_visibility_system と同パターン）。
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
