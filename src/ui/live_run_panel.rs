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
    LiveRuns, TransportCommand, TransportCommandSender, is_terminal_run_status, short_id,
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
const COLOR_PNL_POS: Color = Color::srgb(0.0, 1.0, 0.50);
const COLOR_PNL_NEG: Color = Color::srgb(1.0, 0.20, 0.40);

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

/// Combined run PnL (realized + unrealized) → signed JPY string (§2.8 / §2.9).
/// e.g. `+12,345` / `-980` / `±0`. Whole-yen, thousands-separated, no decimals.
pub fn format_pnl(realized_pnl: f64, unrealized_pnl: f64) -> String {
    let total = realized_pnl + unrealized_pnl;
    // Round to whole yen first so a tiny residual doesn't flip the sign glyph.
    let yen = total.round() as i64;
    let sign = match yen.cmp(&0) {
        std::cmp::Ordering::Greater => "+",
        std::cmp::Ordering::Less => "-",
        std::cmp::Ordering::Equal => "±",
    };
    format!("{sign}{}", group_thousands(yen.unsigned_abs()))
}

/// Color for a combined PnL value: green if > 0, red if < 0, neutral at 0.
pub fn pnl_color(realized_pnl: f64, unrealized_pnl: f64) -> Color {
    let yen = (realized_pnl + unrealized_pnl).round() as i64;
    match yen.cmp(&0) {
        std::cmp::Ordering::Greater => COLOR_PNL_POS,
        std::cmp::Ordering::Less => COLOR_PNL_NEG,
        std::cmp::Ordering::Equal => COLOR_VALUE,
    }
}

/// Order / fill counters → compact `o:<n> f:<m>` cell (§2.8 / §2.9).
pub fn format_counts(order_count: i64, fill_count: i64) -> String {
    format!("o:{order_count} f:{fill_count}")
}

/// Group an unsigned integer with `,` thousands separators (e.g. 12345 → "12,345").
fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let len = bytes.len();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (len - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
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
    /// Combined run PnL (realized + unrealized), JPY, colored by sign.
    Pnl,
    /// Order / fill counters (`o:<n> f:<m>`).
    Counts,
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

impl LiveRunControlAction {
    /// Whether this control may be sent for a run in `status`. Shared by the visual
    /// (greys out) and dispatch (blocks send) systems so the two never drift.
    fn is_allowed(self, status: &str) -> bool {
        match self {
            Self::Pause => can_pause(status),
            Self::Resume => can_resume(status),
            Self::Stop => can_stop(status),
        }
    }
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
                // Stacks below the top-right promote cluster: the Promote button
                // (top 46, h22) and its resident PromoteFeedbackText (top 70, up to
                // ~2 lines for the success "run: <uuid>" line, GlobalZIndex 65).
                // Starting at 72 let that higher-z feedback overprint the panel
                // header on a successful promote — the exact happy path. 108 clears it.
                top: Val::Px(108.0),
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
                    // 2 行目: telemetry (§2.8 / §2.9) — PnL (符号で色分け) / order·fill 数
                    r.spawn((Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        margin: UiRect::top(Val::Px(2.0)),
                        ..default()
                    },))
                        .with_children(|line| {
                            spawn_cell(line, row, LiveRunCell::Pnl, 150.0);
                            spawn_cell(line, row, LiveRunCell::Counts, 120.0);
                        });
                    // 3 行目: 制御ボタン
                    r.spawn((Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::top(Val::Px(2.0)),
                        ..default()
                    },))
                        .with_children(|btns| {
                            spawn_control_button(btns, row, LiveRunControlAction::Pause, "Pause");
                            spawn_control_button(btns, row, LiveRunControlAction::Resume, "Resume");
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
    if !runs.is_changed() {
        return;
    }
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
    if !runs.is_changed() {
        return;
    }
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
    if !runs.is_changed() {
        return;
    }
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
                format!(
                    "{} · {}",
                    short_id(&run.strategy_id, 8),
                    short_id(&run.run_id, 6)
                ),
                COLOR_VALUE,
            ),
            LiveRunCell::Started => (format_hms(run.started_ts_ms), COLOR_VALUE),
            LiveRunCell::Pnl => (
                format!("PnL {}", format_pnl(run.realized_pnl, run.unrealized_pnl)),
                pnl_color(run.realized_pnl, run.unrealized_pnl),
            ),
            LiveRunCell::Counts => (format_counts(run.order_count, run.fill_count), COLOR_VALUE),
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
    if !runs.is_changed() {
        return;
    }
    for (btn, mut bg) in &mut buttons {
        let enabled = runs
            .runs
            .get(btn.row)
            .map(|r| btn.action.is_allowed(&r.status))
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
    interactions: Query<
        (&Interaction, &LiveRunControlButton),
        (Changed<Interaction>, With<Button>),
    >,
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
        if !btn.action.is_allowed(&run.status) {
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
            ..Default::default()
        }
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
    fn format_pnl_signs_and_groups_thousands() {
        assert_eq!(format_pnl(12000.0, 345.0), "+12,345");
        assert_eq!(format_pnl(-980.0, 0.0), "-980");
        assert_eq!(format_pnl(0.0, 0.0), "±0");
        // realized + unrealized combine; rounding to whole yen.
        assert_eq!(format_pnl(1000.4, -0.4), "+1,000");
        assert_eq!(format_pnl(1_234_567.0, 0.0), "+1,234,567");
    }

    #[test]
    fn format_pnl_residual_does_not_flip_sign() {
        // -0.3 rounds to 0 → must read ±0, not -0.
        assert_eq!(format_pnl(-0.3, 0.0), "±0");
    }

    #[test]
    fn pnl_color_tracks_sign() {
        assert_eq!(pnl_color(100.0, 0.0), COLOR_PNL_POS);
        assert_eq!(pnl_color(-100.0, 0.0), COLOR_PNL_NEG);
        assert_eq!(pnl_color(0.0, 0.0), COLOR_VALUE);
    }

    #[test]
    fn format_counts_renders_both() {
        assert_eq!(format_counts(3, 1), "o:3 f:1");
        assert_eq!(format_counts(0, 0), "o:0 f:0");
    }

    #[test]
    fn telemetry_cells_render_from_run_record() {
        let mut app = make_app();
        let mut r = run("RUNNING");
        r.realized_pnl = 5000.0;
        r.unrealized_pnl = 1234.0;
        r.order_count = 4;
        r.fill_count = 2;
        app.world_mut().resource_mut::<LiveRuns>().runs = vec![r];
        app.add_systems(Update, live_run_panel_sync_system);
        let pnl = app
            .world_mut()
            .spawn((
                Text::new(""),
                LiveRunCellTag {
                    row: 0,
                    cell: LiveRunCell::Pnl,
                },
                TextColor(COLOR_VALUE),
            ))
            .id();
        let counts = app
            .world_mut()
            .spawn((
                Text::new(""),
                LiveRunCellTag {
                    row: 0,
                    cell: LiveRunCell::Counts,
                },
                TextColor(COLOR_VALUE),
            ))
            .id();
        app.update();
        assert_eq!(app.world().get::<Text>(pnl).unwrap().0, "PnL +6,234");
        assert_eq!(
            app.world().get::<TextColor>(pnl).unwrap().0,
            COLOR_PNL_POS,
            "positive PnL renders green"
        );
        assert_eq!(app.world().get::<Text>(counts).unwrap().0, "o:4 f:2");
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
