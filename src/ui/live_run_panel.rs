//! Phase 10 §2.8 — Live Run Panel.
//!
//! Lists the active Live Auto run(s) and exposes `[Pause]` / `[Resume]` / `[Stop]`
//! controls. **Bevy UI Node + Interaction** 流派 (it has buttons, so not a
//! world-space panel): spawned once at Startup, the whole panel is `Node.display`
//! -gated on whether any run exists. Rows are driven by the `LiveRuns` resource,
//! which `backend_event_drain_system` fills from `LiveStrategyEvent` pushes.
//!
//! Phase 10 caps automated runs to 1, but the panel renders a small fixed set of
//! rows (forward-compatible with Phase 11 multi-run). Run-level PnL / order / fill
//! telemetry is NOT shown here yet — that needs a telemetry event (Step 7 / §2.9);
//! Step 6 shows lifecycle status + start time + the controls.

use bevy::prelude::*;
use chrono::{Local, TimeZone};

use crate::trading::{
    LiveRuns, TransportCommand, TransportCommandSender, is_terminal_run_status,
};

const MAX_PANEL_ROWS: usize = 3;

// ── 配色 ───────────────────────────────────────────────────────────────────
const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.96);
const COLOR_HEADER: Color = Color::srgb(1.0, 0.55, 0.0);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_RUNNING: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_PAUSED: Color = Color::srgb(1.0, 0.78, 0.0);
const COLOR_ERROR: Color = Color::srgb(1.0, 0.20, 0.40);
const COLOR_STOPPED: Color = Color::srgb(0.55, 0.55, 0.55);
const COLOR_BTN_IDLE: Color = Color::srgba(0.18, 0.20, 0.28, 1.0);
const COLOR_BTN_STOP: Color = Color::srgba(0.30, 0.16, 0.20, 1.0);
const COLOR_BTN_DISABLED: Color = Color::srgba(0.12, 0.12, 0.16, 1.0);

// ===========================================================================
// Pure helpers (testable)
// ===========================================================================

/// 状態文字列に応じた表示色。
pub fn status_color(status: &str) -> Color {
    match status {
        "RUNNING" => COLOR_RUNNING,
        "PAUSED" => COLOR_PAUSED,
        "ERROR" => COLOR_ERROR,
        "STOPPED" | "STOPPING" => COLOR_STOPPED,
        _ => COLOR_VALUE,
    }
}

/// epoch ms → ローカル `HH:MM:SS`。0 / 不正は "—"。
pub fn format_hms(ts_ms: i64) -> String {
    if ts_ms <= 0 {
        return "—".to_string();
    }
    match Local.timestamp_millis_opt(ts_ms).single() {
        Some(dt) => dt.format("%H:%M:%S").to_string(),
        None => "—".to_string(),
    }
}

/// id の末尾 `n` 文字（短縮表示用）。短い id はそのまま。
pub fn short_id(id: &str, n: usize) -> String {
    let count = id.chars().count();
    if count <= n {
        return id.to_string();
    }
    let tail: String = id.chars().skip(count - n).collect();
    format!("…{tail}")
}

/// Pause を送れる状態か（RUNNING のみ）。
pub fn can_pause(status: &str) -> bool {
    status == "RUNNING"
}

/// Resume を送れる状態か（PAUSED のみ）。
pub fn can_resume(status: &str) -> bool {
    status == "PAUSED"
}

/// Stop を送れる状態か（非終端なら常に可 — runaway を止められるように）。
pub fn can_stop(status: &str) -> bool {
    !is_terminal_run_status(status)
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct LiveRunPanelRoot;

/// 行コンテナ。`index` は `LiveRuns.runs` への添字。run 数を超える行は Display で隠す。
#[derive(Component, Clone, Copy)]
pub struct LiveRunRow {
    pub index: usize,
}

#[derive(Component, Clone, Copy)]
pub enum LiveRunCell {
    Status,
    Ids,
    Started,
}

#[derive(Component, Clone, Copy)]
pub struct LiveRunCellTag {
    pub row: usize,
    pub cell: LiveRunCell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRunControlAction {
    Pause,
    Resume,
    Stop,
}

#[derive(Component, Clone, Copy)]
pub struct LiveRunControlButton {
    pub row: usize,
    pub action: LiveRunControlAction,
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

fn spawn_control_button(
    parent: &mut ChildBuilder,
    row: usize,
    action: LiveRunControlAction,
    label: &str,
) {
    let bg = match action {
        LiveRunControlAction::Stop => COLOR_BTN_STOP,
        _ => COLOR_BTN_IDLE,
    };
    parent
        .spawn((
            Button,
            Node {
                height: Val::Px(18.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                margin: UiRect::left(Val::Px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(bg),
            LiveRunControlButton { row, action },
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(COLOR_VALUE),
            ));
        });
}

fn spawn_cell(parent: &mut ChildBuilder, row: usize, cell: LiveRunCell, width: f32) {
    parent.spawn((
        Node {
            width: Val::Px(width),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(COLOR_VALUE),
        LiveRunCellTag { row, cell },
    ));
}

/// Live Run Panel 本体を spawn する (Startup)。初期 Display は None。
pub fn spawn_live_run_panel(mut commands: Commands) {
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(72.0),
                right: Val::Px(12.0),
                width: Val::Px(304.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(4.0),
                ..default()
            },
            BackgroundColor(COLOR_PANEL_BG),
            GlobalZIndex(62),
            LiveRunPanelRoot,
            Name::new("LiveRunPanel"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                },
                Text::new("LIVE RUNS"),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(COLOR_HEADER),
            ));
            for row in 0..MAX_PANEL_ROWS {
                p.spawn((
                    Node {
                        display: Display::None,
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        margin: UiRect::bottom(Val::Px(4.0)),
                        ..default()
                    },
                    LiveRunRow { index: row },
                ))
                .with_children(|r| {
                    // 1 行目: status / ids / started
                    r.spawn((Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        ..default()
                    },))
                        .with_children(|line| {
                            spawn_cell(line, row, LiveRunCell::Status, 70.0);
                            spawn_cell(line, row, LiveRunCell::Ids, 130.0);
                            spawn_cell(line, row, LiveRunCell::Started, 70.0);
                        });
                    // 2 行目: 制御ボタン
                    r.spawn((Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::top(Val::Px(2.0)),
                        ..default()
                    },))
                        .with_children(|btns| {
                            spawn_control_button(btns, row, LiveRunControlAction::Pause, "Pause");
                            spawn_control_button(
                                btns,
                                row,
                                LiveRunControlAction::Resume,
                                "Resume",
                            );
                            spawn_control_button(btns, row, LiveRunControlAction::Stop, "Stop");
                        });
                });
            }
        });
}

// ===========================================================================
// Systems
// ===========================================================================

/// パネル root の Display を「run が 1 件以上あるか」に同期する。
pub fn live_run_panel_visibility_system(
    runs: Res<LiveRuns>,
    mut root_q: Query<&mut Node, With<LiveRunPanelRoot>>,
) {
    let target = if runs.runs.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    for mut node in &mut root_q {
        if node.display != target {
            node.display = target;
        }
    }
}

/// 行コンテナの Display を run 数に同期する（超過行は隠す）。
pub fn live_run_row_visibility_system(
    runs: Res<LiveRuns>,
    mut rows: Query<(&LiveRunRow, &mut Node)>,
) {
    for (row, mut node) in &mut rows {
        let target = if row.index < runs.runs.len() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != target {
            node.display = target;
        }
    }
}

/// 各セルのテキスト/色を差分反映する。
pub fn live_run_panel_sync_system(
    runs: Res<LiveRuns>,
    mut cells: Query<(&LiveRunCellTag, &mut Text, &mut TextColor)>,
) {
    for (tag, mut text, mut color) in &mut cells {
        let Some(run) = runs.runs.get(tag.row) else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        let (new_text, new_color) = match tag.cell {
            LiveRunCell::Status => (run.status.clone(), status_color(&run.status)),
            LiveRunCell::Ids => (
                format!("{} · {}", short_id(&run.strategy_id, 8), short_id(&run.run_id, 6)),
                COLOR_VALUE,
            ),
            LiveRunCell::Started => (format_hms(run.started_ts_ms), COLOR_VALUE),
        };
        if text.0 != new_text {
            text.0 = new_text;
        }
        if color.0 != new_color {
            color.0 = new_color;
        }
    }
}

/// 制御ボタンの有効/無効に応じて背景色を差分反映する（無効ボタンはグレー）。
pub fn live_run_control_visual_system(
    runs: Res<LiveRuns>,
    mut buttons: Query<(&LiveRunControlButton, &mut BackgroundColor)>,
) {
    for (btn, mut bg) in &mut buttons {
        let enabled = runs
            .runs
            .get(btn.row)
            .map(|r| match btn.action {
                LiveRunControlAction::Pause => can_pause(&r.status),
                LiveRunControlAction::Resume => can_resume(&r.status),
                LiveRunControlAction::Stop => can_stop(&r.status),
            })
            .unwrap_or(false);
        let target = if !enabled {
            COLOR_BTN_DISABLED
        } else if btn.action == LiveRunControlAction::Stop {
            COLOR_BTN_STOP
        } else {
            COLOR_BTN_IDLE
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

/// 制御ボタン押下を run_id 付きの `Pause/Resume/StopLiveStrategy` に変換して送る。
/// 状態が許さない遷移（PAUSED でない run の Pause 等）と終端 run は送らない。
pub fn live_run_control_button_system(
    interactions: Query<(&Interaction, &LiveRunControlButton), (Changed<Interaction>, With<Button>)>,
    runs: Res<LiveRuns>,
    sender: Option<Res<TransportCommandSender>>,
) {
    for (interaction, btn) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(run) = runs.runs.get(btn.row) else {
            continue;
        };
        let allowed = match btn.action {
            LiveRunControlAction::Pause => can_pause(&run.status),
            LiveRunControlAction::Resume => can_resume(&run.status),
            LiveRunControlAction::Stop => can_stop(&run.status),
        };
        if !allowed {
            continue;
        }
        let run_id = run.run_id.clone();
        let cmd = match btn.action {
            LiveRunControlAction::Pause => TransportCommand::PauseLiveStrategy { run_id },
            LiveRunControlAction::Resume => TransportCommand::ResumeLiveStrategy { run_id },
            LiveRunControlAction::Stop => TransportCommand::StopLiveStrategy { run_id },
        };
        match sender.as_ref() {
            Some(tx) => {
                let _ = tx.tx.send(cmd);
            }
            None => warn!("live-run control skipped: TransportCommandSender unavailable"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::LiveRunRecord;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<LiveRuns>();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().spawn(RxHolder { _rx: rx });
        app
    }

    #[derive(Component)]
    struct RxHolder {
        _rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    }

    fn run(status: &str) -> LiveRunRecord {
        LiveRunRecord {
            run_id: "run-abc123".to_string(),
            strategy_id: "strat-deadbeef0011".to_string(),
            status: status.to_string(),
            started_ts_ms: 1,
            updated_ts_ms: 1,
        }
    }

    #[test]
    fn short_id_keeps_short_and_truncates_long() {
        assert_eq!(short_id("abc", 6), "abc");
        assert_eq!(short_id("strat-deadbeef0011", 8), "…beef0011");
        assert_eq!(short_id("strat-deadbeef0011", 8).chars().count(), 9); // ellipsis + 8
    }

    #[test]
    fn control_gating_matches_status() {
        assert!(can_pause("RUNNING") && !can_pause("PAUSED"));
        assert!(can_resume("PAUSED") && !can_resume("RUNNING"));
        assert!(can_stop("RUNNING") && can_stop("PAUSED"));
        assert!(!can_stop("STOPPED") && !can_stop("ERROR"));
    }

    #[test]
    fn format_hms_handles_zero() {
        assert_eq!(format_hms(0), "—");
        assert_eq!(format_hms(-5), "—");
        assert_ne!(format_hms(1_700_000_000_000), "—");
    }

    #[test]
    fn pause_button_fires_for_running_run() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<LiveRuns>().runs = vec![run("RUNNING")];
        app.add_systems(Update, live_run_control_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            LiveRunControlButton {
                row: 0,
                action: LiveRunControlAction::Pause,
            },
        ));
        app.update();
        match rx.try_recv().expect("Pause must fire") {
            TransportCommand::PauseLiveStrategy { run_id } => assert_eq!(run_id, "run-abc123"),
            other => panic!("expected PauseLiveStrategy, got {other:?}"),
        }
    }

    #[test]
    fn pause_button_is_noop_when_not_running() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<LiveRuns>().runs = vec![run("PAUSED")];
        app.add_systems(Update, live_run_control_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            LiveRunControlButton {
                row: 0,
                action: LiveRunControlAction::Pause,
            },
        ));
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "Pause must not fire for a non-RUNNING run"
        );
    }

    #[test]
    fn stop_button_fires_for_paused_run() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<LiveRuns>().runs = vec![run("PAUSED")];
        app.add_systems(Update, live_run_control_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            LiveRunControlButton {
                row: 0,
                action: LiveRunControlAction::Stop,
            },
        ));
        app.update();
        match rx.try_recv().expect("Stop must fire for an active run") {
            TransportCommand::StopLiveStrategy { run_id } => assert_eq!(run_id, "run-abc123"),
            other => panic!("expected StopLiveStrategy, got {other:?}"),
        }
    }

    #[test]
    fn stop_button_is_noop_for_terminal_run() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<LiveRuns>().runs = vec![run("STOPPED")];
        app.add_systems(Update, live_run_control_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            LiveRunControlButton {
                row: 0,
                action: LiveRunControlAction::Stop,
            },
        ));
        app.update();
        assert!(rx.try_recv().is_err(), "terminal run must not be stoppable");
    }
}
