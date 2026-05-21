//! Phase 10 §2.7 / §0.2 — Promote-to-Live trigger + Safety Rails modal.
//!
//! This is the UI front door to Live Auto strategy execution. It owns two pieces,
//! both **Bevy UI Node + Interaction** 流派 (like `order_panel.rs` / `secret_modal.rs`),
//! spawned once at Startup and toggled with `Node.display`:
//!
//! 1. A `[Promote to Live]` trigger button (gated/greyed on a pre-flight check:
//!    a strategy must be loaded and the venue must be connected). On click it
//!    flushes the editor to its cache `.py` (same path Replay's Run uses), captures
//!    the promote context, and opens the modal.
//! 2. The Safety Rails modal: shows the latest Replay KPI summary (existing
//!    `summary.py` fields only — total_pnl / fills / equity points, §5 M3),
//!    exposes the four numeric safety limits as ± steppers (`0 = disabled`,
//!    mirroring the backend `SafetyRails`), and confirms with a single
//!    `TransportCommand::PromoteToLive` (the transport task chains
//!    Register → SetExecutionMode(LiveAuto) → Start).
//!
//! Why ± steppers and not text fields: the limits are round numbers and the
//! proven `order_panel` qty/price stepper avoids multi-field keyboard-focus
//! management. Why a dedicated module (not `strategy_editor.rs`): the editor is a
//! world-space cosmic panel; an Interaction/Button belongs in the UI-node layer,
//! and keeping the whole feature in one lib module avoids touching the editor.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::trading::{
    LastRunResult, PromoteFeedback, SafetyLimitsInput, SecretPrompt, SelectedSymbol,
    TransportCommand, TransportCommandSender, VenueState, VenueStatusRes, is_venue_live,
};
use crate::ui::components::{StrategyBuffer, StrategyEditorId, StrategyFragment, WindowRoot};
use crate::ui::strategy_editor::{StrategyAutoSaveState, flush_strategy_cache, merge_fragments};

// ── 配色 ───────────────────────────────────────────────────────────────────
const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.98);
const COLOR_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const COLOR_HEADER: Color = Color::srgb(1.0, 0.55, 0.0); // amber: real-money path
const COLOR_LABEL: Color = Color::srgb(0.65, 0.70, 0.78);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_BTN_IDLE: Color = Color::srgba(0.18, 0.20, 0.28, 1.0);
const COLOR_BTN_SUBMIT: Color = Color::srgba(0.55, 0.30, 0.05, 1.0); // amber confirm
const COLOR_BTN_CANCEL: Color = Color::srgba(0.30, 0.16, 0.20, 1.0);
const COLOR_TRIGGER_IDLE: Color = Color::srgba(0.45, 0.25, 0.05, 1.0);
const COLOR_TRIGGER_HOVER: Color = Color::srgba(0.60, 0.34, 0.08, 1.0);
const COLOR_TRIGGER_DISABLED: Color = Color::srgba(0.16, 0.16, 0.20, 1.0);

// ── 安全レール ステッパーの刻み幅 (§0.6) ───────────────────────────────────
/// JPY 系上限の ± 刻み (max_position / max_order_value / max_daily_loss)。
const JPY_STEP: i64 = 100_000;
/// orders/min の ± 刻み。
const RATE_STEP: i32 = 1;

// ===========================================================================
// Resources
// ===========================================================================

/// 起動中の Promote フロー。`active` が `Some` の間だけ Safety Rails モーダルを出す。
/// 起動ボタンが pre-flight を通したときに `PromoteContext` をセットし、
/// Confirm / Cancel が `None` に戻す。
#[derive(Resource, Default, Debug, Clone)]
pub struct PromotePrompt {
    pub active: Option<PromoteContext>,
}

/// Confirm 時にそのまま `PromoteToLive` へ詰める確定済みコンテキスト。
/// trigger ボタンが「保存済み .py path / 対象銘柄 / venue」を確定して作る。
#[derive(Debug, Clone, PartialEq)]
pub struct PromoteContext {
    pub strategy_file: std::path::PathBuf,
    pub instrument_id: String,
    pub venue: String,
}

/// Safety Rails の現在の入力値。`0` はそのレール無効 (§0.6 / backend `SafetyRails`)。
/// Default は §0.6 の既定値。
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct SafetyRailsForm {
    pub max_position_size_jpy: i64,
    pub max_order_value_jpy: i64,
    pub max_daily_loss_jpy: i64,
    pub max_orders_per_minute: i32,
}

impl Default for SafetyRailsForm {
    fn default() -> Self {
        Self {
            max_position_size_jpy: 1_000_000,
            max_order_value_jpy: 500_000,
            max_daily_loss_jpy: 100_000,
            max_orders_per_minute: 5,
        }
    }
}

// ===========================================================================
// Pure helpers (testable without an App)
// ===========================================================================

/// Promote の pre-flight (§0.2)。ブロック理由があれば `Some(理由)`、OK なら `None`。
/// `strategy_loaded` は editor に保存先 (`cache_path`) があるか。
pub fn preflight_blocker(
    venue_state: VenueState,
    strategy_loaded: bool,
    instrument_selected: bool,
) -> Option<&'static str> {
    if !strategy_loaded {
        return Some("戦略が未ロードです");
    }
    if !is_venue_live(venue_state) {
        return Some("venue に未接続です (ログインしてください)");
    }
    if !instrument_selected {
        return Some("対象銘柄が未選択です");
    }
    None
}

/// 上限値の表示文字列。`0` は無効化を意味するので "OFF"。
pub fn format_limit_jpy(value: i64) -> String {
    if value <= 0 {
        "OFF".to_string()
    } else {
        format!("¥{value}")
    }
}

/// orders/min の表示文字列。`0` は無効。
pub fn format_rate(value: i32) -> String {
    if value <= 0 {
        "OFF".to_string()
    } else {
        format!("{value}/min")
    }
}

/// フォーム + 対象銘柄から transport 用の `SafetyLimitsInput` を組む。
/// `allowed_instruments` は pre-trade ホワイトリストで、Phase 10 の単一銘柄 run では
/// 起動対象の銘柄 1 件に固定する (§0.6)。空銘柄なら空リスト (＝全許可) にフォールバック。
pub fn build_safety_limits(form: &SafetyRailsForm, instrument_id: &str) -> SafetyLimitsInput {
    let allowed_instruments = if instrument_id.is_empty() {
        Vec::new()
    } else {
        vec![instrument_id.to_string()]
    };
    SafetyLimitsInput {
        max_position_size_jpy: form.max_position_size_jpy.max(0),
        max_order_value_jpy: form.max_order_value_jpy.max(0),
        max_daily_loss_jpy: form.max_daily_loss_jpy.max(0),
        max_orders_per_minute: form.max_orders_per_minute.max(0),
        allowed_instruments,
    }
}

/// Replay KPI サマリー文字列。既存 `summary.py` が算出する項目のみ (§5 M3:
/// Sharpe / 累積リターン% は未算出なので出さない)。Replay 未実行なら注記を返す。
pub fn replay_kpi_summary(last_run: &LastRunResult) -> String {
    match &last_run.parsed_summary {
        Some(s) => {
            let run = last_run.run_id.as_deref().unwrap_or("—");
            format!(
                "直近 Replay: pnl {:.0} / fills {} / eq_pts {} / {}\nrun: {}",
                s.total_pnl, s.fills_count, s.equity_points, s.status, run
            )
        }
        None => "直近 Replay 結果なし (Promote 前に Replay 実行を推奨)".to_string(),
    }
}

// ===========================================================================
// Components
// ===========================================================================

/// 起動ボタン (UI-node、常駐、pre-flight で enabled/disabled)。
#[derive(Component)]
pub struct PromoteTriggerButton;

/// 起動ボタン直下の常駐フィードバック行。`PromoteFeedback.message` を表示する。
/// モーダルは Confirm で閉じるため、RPC chain の async な成功/拒否はモーダル内では
/// 出せない。常駐するこの行に出すことで pre-flight ブロック理由・「起動中…」・
/// 起動成功 (run id)・構造化 reject (error_code) のすべてをユーザーに surface する。
#[derive(Component)]
pub struct PromoteFeedbackText;

#[derive(Component)]
pub struct SafetyRailsModalRoot;

/// ± ステッパー。各 JPY レールは `JPY_STEP`、rate は `RATE_STEP` 刻み。
#[derive(Component, Clone, Copy)]
pub enum SafetyRailsStepper {
    PositionDec,
    PositionInc,
    OrderValueDec,
    OrderValueInc,
    DailyLossDec,
    DailyLossInc,
    OrdersPerMinDec,
    OrdersPerMinInc,
}

#[derive(Component, Clone, Copy)]
pub enum SafetyRailsModalButton {
    Confirm,
    Cancel,
}

/// モーダル内で sync system が値を書き込むテキストノードの識別子。
#[derive(Component, Clone, Copy)]
pub enum SafetyRailsField {
    Position,
    OrderValue,
    DailyLoss,
    OrdersPerMin,
    Context,
    Kpi,
    Allowed,
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

/// 起動ボタンを spawn する (Startup)。常駐し、pre-flight で色/有効性が変わる。
pub fn spawn_promote_trigger(mut commands: Commands) {
    commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(46.0),
                right: Val::Px(12.0),
                height: Val::Px(22.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_TRIGGER_DISABLED),
            GlobalZIndex(65),
            PromoteTriggerButton,
            Name::new("PromoteToLiveButton"),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new("Promote to Live ▶"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(COLOR_VALUE),
            ));
        });
    // 起動ボタン直下の常駐フィードバック行 (right 12px に右端を固定)。
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(70.0),
            right: Val::Px(12.0),
            max_width: Val::Px(320.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: 11.0,
            ..default()
        },
        TextColor(COLOR_HEADER),
        GlobalZIndex(65),
        PromoteFeedbackText,
        Name::new("PromoteFeedbackText"),
    ));
}

fn spawn_stepper(parent: &mut ChildBuilder, action: SafetyRailsStepper, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Px(22.0),
                height: Val::Px(20.0),
                margin: UiRect::horizontal(Val::Px(3.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_BTN_IDLE),
            action,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(COLOR_VALUE),
            ));
        });
}

/// `Label  [-] value [+]` の 1 レール行を組む。
fn spawn_rail_row(
    parent: &mut ChildBuilder,
    label: &str,
    field: SafetyRailsField,
    dec: SafetyRailsStepper,
    inc: SafetyRailsStepper,
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(5.0)),
            align_items: AlignItems::Center,
            ..default()
        },))
        .with_children(|r| {
            r.spawn((
                Node {
                    width: Val::Px(120.0),
                    ..default()
                },
                Text::new(label.to_string()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(COLOR_LABEL),
            ));
            spawn_stepper(r, dec, "-");
            r.spawn((
                Node {
                    width: Val::Px(96.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(COLOR_VALUE),
                field,
            ));
            spawn_stepper(r, inc, "+");
        });
}

/// Safety Rails モーダル本体を spawn する (Startup)。初期 Display は None。
pub fn spawn_safety_rails_modal(mut commands: Commands) {
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_BACKDROP),
            // 注文確認 (200) より前面、secret (300) より背面。
            GlobalZIndex(250),
            SafetyRailsModalRoot,
            Name::new("SafetyRailsModal"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(380.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
            ))
            .with_children(|card| {
                card.spawn((
                    Node {
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                    Text::new("PROMOTE TO LIVE — Safety Rails"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(COLOR_HEADER),
                ));
                // 対象 (戦略 / 銘柄 / venue)
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::bottom(Val::Px(6.0)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    SafetyRailsField::Context,
                ));
                // Replay KPI サマリー (既存 summary.py 項目のみ)
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::bottom(Val::Px(10.0)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_LABEL),
                    SafetyRailsField::Kpi,
                ));
                // レール ステッパー (0 = OFF)
                spawn_rail_row(
                    card,
                    "Max Order Value:",
                    SafetyRailsField::OrderValue,
                    SafetyRailsStepper::OrderValueDec,
                    SafetyRailsStepper::OrderValueInc,
                );
                spawn_rail_row(
                    card,
                    "Max Position:",
                    SafetyRailsField::Position,
                    SafetyRailsStepper::PositionDec,
                    SafetyRailsStepper::PositionInc,
                );
                spawn_rail_row(
                    card,
                    "Max Daily Loss:",
                    SafetyRailsField::DailyLoss,
                    SafetyRailsStepper::DailyLossDec,
                    SafetyRailsStepper::DailyLossInc,
                );
                spawn_rail_row(
                    card,
                    "Max Orders/min:",
                    SafetyRailsField::OrdersPerMin,
                    SafetyRailsStepper::OrdersPerMinDec,
                    SafetyRailsStepper::OrdersPerMinInc,
                );
                // allowed_instruments (読み取り専用、起動銘柄に固定)
                card.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        margin: UiRect::vertical(Val::Px(4.0)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_LABEL),
                    SafetyRailsField::Allowed,
                ));
                // ボタン行
                card.spawn((Node {
                    margin: UiRect::top(Val::Px(12.0)),
                    column_gap: Val::Px(10.0),
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(30.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(COLOR_BTN_CANCEL),
                            SafetyRailsModalButton::Cancel,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("キャンセル"),
                                TextFont {
                                    font_size: 13.0,
                                    ..default()
                                },
                                TextColor(COLOR_VALUE),
                            ));
                        });
                        btns.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                height: Val::Px(30.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(COLOR_BTN_SUBMIT),
                            SafetyRailsModalButton::Confirm,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("Live 起動"),
                                TextFont {
                                    font_size: 13.0,
                                    ..default()
                                },
                                TextColor(COLOR_VALUE),
                            ));
                        });
                    });
            });
        });
}

// ===========================================================================
// Systems
// ===========================================================================

/// 起動ボタンの色を pre-flight 結果に同期する (enabled=amber / blocked=grey)。
/// ホバーは enabled のときだけ反応する。
#[allow(clippy::type_complexity)]
pub fn promote_trigger_visual_system(
    mut q: Query<(&Interaction, &mut BackgroundColor), With<PromoteTriggerButton>>,
    venue: Res<VenueStatusRes>,
    buffer: Res<StrategyBuffer>,
    selected: Res<SelectedSymbol>,
) {
    let blocked = preflight_blocker(
        venue.state,
        buffer.cache_path.is_some(),
        selected.id.is_some(),
    )
    .is_some();
    for (interaction, mut bg) in &mut q {
        let target = if blocked {
            COLOR_TRIGGER_DISABLED
        } else {
            match interaction {
                Interaction::Hovered | Interaction::Pressed => COLOR_TRIGGER_HOVER,
                Interaction::None => COLOR_TRIGGER_IDLE,
            }
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

/// 起動ボタン押下: pre-flight → editor を cache へ flush → コンテキスト確定 → モーダルを開く。
/// pre-flight 失敗時は `PromoteFeedback` に理由を出すだけで開かない (§0.2 disabled 相当)。
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
pub fn promote_trigger_button_system(
    interactions: Query<(&Interaction, &PromoteTriggerButton), Changed<Interaction>>,
    venue: Res<VenueStatusRes>,
    selected: Res<SelectedSymbol>,
    mut buffer: ResMut<StrategyBuffer>,
    mut auto_save: ResMut<StrategyAutoSaveState>,
    fragments_q: Query<(&StrategyEditorId, &StrategyFragment), With<WindowRoot>>,
    mut prompt: ResMut<PromotePrompt>,
    mut feedback: ResMut<PromoteFeedback>,
) {
    for (interaction, _) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        // 既に開いているなら二重 open を無視。
        if prompt.active.is_some() {
            continue;
        }
        if let Some(reason) = preflight_blocker(
            venue.state,
            buffer.cache_path.is_some(),
            selected.id.is_some(),
        ) {
            feedback.message = Some(format!("Promote 不可: {reason}"));
            continue;
        }
        // Run と同じ手順で merge → flush し、保存済みの canonical .py path を得る。
        let mut items: Vec<(String, String)> = fragments_q
            .iter()
            .map(|(id, f)| (id.region_key.clone(), f.source.clone()))
            .collect();
        items.sort_by(|a, b| a.0.cmp(&b.0));
        let merged = merge_fragments(&items);
        match flush_strategy_cache(&merged, &mut buffer, &mut auto_save) {
            Ok(true) => {}
            Ok(false) => {
                feedback.message = Some("Promote 不可: 保存先が未設定です".to_string());
                continue;
            }
            Err(e) => {
                feedback.message = Some(format!("Promote 不可: 保存に失敗 ({e})"));
                continue;
            }
        }
        let Some(strategy_file) = buffer.cache_path.clone() else {
            feedback.message = Some("Promote 不可: 保存先が未設定です".to_string());
            continue;
        };
        // pre-flight が銘柄選択を保証済み。venue_id は接続中なら Some。
        let instrument_id = selected.id.clone().unwrap_or_default();
        let venue_id = venue.venue_id.clone().unwrap_or_default();
        feedback.message = None;
        prompt.active = Some(PromoteContext {
            strategy_file,
            instrument_id,
            venue: venue_id,
        });
    }
}

/// モーダル root の Display を `PromotePrompt.active` に同期する。
pub fn safety_rails_modal_visibility_system(
    prompt: Res<PromotePrompt>,
    mut root_q: Query<&mut Node, With<SafetyRailsModalRoot>>,
) {
    let target = if prompt.active.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut root_q {
        if node.display != target {
            node.display = target;
        }
    }
}

/// ± ステッパー押下を `SafetyRailsForm` に反映する (0 で下限クランプ)。
/// モーダルが閉じている間は無視 (背後のステッパーへの誤爆防止)。
pub fn safety_rails_stepper_system(
    interactions: Query<(&Interaction, &SafetyRailsStepper), (Changed<Interaction>, With<Button>)>,
    prompt: Res<PromotePrompt>,
    mut form: ResMut<SafetyRailsForm>,
) {
    if prompt.active.is_none() {
        return;
    }
    for (interaction, stepper) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match stepper {
            SafetyRailsStepper::PositionDec => {
                form.max_position_size_jpy = (form.max_position_size_jpy - JPY_STEP).max(0);
            }
            SafetyRailsStepper::PositionInc => form.max_position_size_jpy += JPY_STEP,
            SafetyRailsStepper::OrderValueDec => {
                form.max_order_value_jpy = (form.max_order_value_jpy - JPY_STEP).max(0);
            }
            SafetyRailsStepper::OrderValueInc => form.max_order_value_jpy += JPY_STEP,
            SafetyRailsStepper::DailyLossDec => {
                form.max_daily_loss_jpy = (form.max_daily_loss_jpy - JPY_STEP).max(0);
            }
            SafetyRailsStepper::DailyLossInc => form.max_daily_loss_jpy += JPY_STEP,
            SafetyRailsStepper::OrdersPerMinDec => {
                form.max_orders_per_minute = (form.max_orders_per_minute - RATE_STEP).max(0);
            }
            SafetyRailsStepper::OrdersPerMinInc => form.max_orders_per_minute += RATE_STEP,
        }
    }
}

/// `[Live 起動]` → `TransportCommand::PromoteToLive` 発射 + prompt クローズ。
/// `[キャンセル]` / Esc → prompt クローズ (何も送らない)。
pub fn safety_rails_modal_button_system(
    interactions: Query<
        (&Interaction, &SafetyRailsModalButton),
        (Changed<Interaction>, With<Button>),
    >,
    keys: Res<ButtonInput<KeyCode>>,
    secret_prompt: Res<SecretPrompt>,
    form: Res<SafetyRailsForm>,
    mut prompt: ResMut<PromotePrompt>,
    mut feedback: ResMut<PromoteFeedback>,
    sender: Option<Res<TransportCommandSender>>,
) {
    // 開いていなければ何もしない (stray Pressed で誤発注しないためのガード)。
    if prompt.active.is_none() {
        return;
    }
    // Esc = キャンセル。§3.10 Escape determinism: 最前面の SecretModal が開いているときは
    // そちらに Esc を譲る (この system は `.before(secret_modal_input_system)` で走り、secret の
    // drain が prompt をまだ閉じていない時点の `active` を読む)。実運用では promote と secret は
    // 時系列が重ならないが、他モーダルと挙動を揃えて 1 打鍵で両方閉じる事故を防ぐ。
    if keys.just_pressed(KeyCode::Escape) && secret_prompt.active.is_none() {
        prompt.active = None;
        return;
    }
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            SafetyRailsModalButton::Cancel => {
                prompt.active = None;
            }
            SafetyRailsModalButton::Confirm => {
                let Some(ctx) = prompt.active.take() else {
                    continue;
                };
                let safety_limits = build_safety_limits(&form, &ctx.instrument_id);
                match sender.as_ref() {
                    Some(tx) => {
                        let _ = tx.tx.send(TransportCommand::PromoteToLive {
                            strategy_file: ctx.strategy_file,
                            expected_sha256: String::new(),
                            instrument_id: ctx.instrument_id,
                            venue: ctx.venue,
                            params: HashMap::new(),
                            safety_limits,
                            ensure_live_auto: true,
                        });
                        feedback.message = Some("Live 戦略を起動中…".to_string());
                    }
                    None => {
                        warn!("PromoteToLive skipped: TransportCommandSender unavailable");
                        feedback.message = Some("Promote 不可: backend 未接続".to_string());
                    }
                }
            }
        }
    }
}

/// モーダル内の値テキスト (レール値 / KPI / 対象 / allowed) を差分反映する。
#[allow(clippy::type_complexity)]
pub fn safety_rails_modal_sync_system(
    prompt: Res<PromotePrompt>,
    form: Res<SafetyRailsForm>,
    last_run: Res<LastRunResult>,
    mut fields: Query<(&SafetyRailsField, &mut Text)>,
) {
    let ctx = prompt.active.as_ref();
    for (field, mut text) in &mut fields {
        let new = match field {
            SafetyRailsField::Position => format_limit_jpy(form.max_position_size_jpy),
            SafetyRailsField::OrderValue => format_limit_jpy(form.max_order_value_jpy),
            SafetyRailsField::DailyLoss => format_limit_jpy(form.max_daily_loss_jpy),
            SafetyRailsField::OrdersPerMin => format_rate(form.max_orders_per_minute),
            SafetyRailsField::Context => match ctx {
                Some(c) => format!("銘柄: {} / venue: {}", c.instrument_id, c.venue),
                None => String::new(),
            },
            SafetyRailsField::Kpi => replay_kpi_summary(&last_run),
            SafetyRailsField::Allowed => match ctx {
                Some(c) if !c.instrument_id.is_empty() => {
                    format!("発注許可銘柄: {} のみ", c.instrument_id)
                }
                _ => "発注許可銘柄: (制限なし)".to_string(),
            },
        };
        if text.0 != new {
            text.0 = new;
        }
    }
    let _ = last_run.state; // RunState は KPI 文字列に含めない (Step 6 panel が担当)
}

/// `PromoteFeedback.message` を起動ボタン直下の常駐行に差分反映する。
/// pre-flight ブロック理由・「起動中…」・起動成功/拒否 (RPC chain の async 結果) を
/// surface する唯一の経路 (モーダルは Confirm で閉じるため async 結果を出せない)。
pub fn promote_feedback_sync_system(
    feedback: Res<PromoteFeedback>,
    mut q: Query<&mut Text, With<PromoteFeedbackText>>,
) {
    let msg = feedback.message.as_deref().unwrap_or_default();
    for mut text in &mut q {
        if text.0 != msg {
            text.0 = msg.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<PromotePrompt>();
        app.init_resource::<SafetyRailsForm>();
        app.init_resource::<PromoteFeedback>();
        app.init_resource::<SecretPrompt>();
        app.init_resource::<ButtonInput<KeyCode>>();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().spawn(RxHolder { _rx: rx });
        app
    }

    #[derive(Component)]
    struct RxHolder {
        _rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    }

    fn open_prompt(app: &mut App) {
        app.world_mut().resource_mut::<PromotePrompt>().active = Some(PromoteContext {
            strategy_file: std::path::PathBuf::from("/tmp/strat.py"),
            instrument_id: "7203.T".to_string(),
            venue: "MOCK".to_string(),
        });
    }

    #[test]
    fn preflight_blocks_without_strategy() {
        assert_eq!(
            preflight_blocker(VenueState::Connected, false, true),
            Some("戦略が未ロードです")
        );
    }

    #[test]
    fn preflight_blocks_without_venue() {
        assert_eq!(
            preflight_blocker(VenueState::Disconnected, true, true),
            Some("venue に未接続です (ログインしてください)")
        );
        // Subscribed counts as live.
        assert_eq!(preflight_blocker(VenueState::Subscribed, true, true), None);
    }

    #[test]
    fn preflight_blocks_without_instrument() {
        assert_eq!(
            preflight_blocker(VenueState::Connected, true, false),
            Some("対象銘柄が未選択です")
        );
    }

    #[test]
    fn preflight_passes_when_all_ok() {
        assert_eq!(preflight_blocker(VenueState::Connected, true, true), None);
    }

    #[test]
    fn format_limit_marks_zero_as_off() {
        assert_eq!(format_limit_jpy(0), "OFF");
        assert_eq!(format_limit_jpy(500_000), "¥500000");
        assert_eq!(format_rate(0), "OFF");
        assert_eq!(format_rate(5), "5/min");
    }

    #[test]
    fn build_safety_limits_whitelists_single_instrument() {
        let form = SafetyRailsForm::default();
        let limits = build_safety_limits(&form, "7203.T");
        assert_eq!(limits.allowed_instruments, vec!["7203.T".to_string()]);
        assert_eq!(limits.max_order_value_jpy, 500_000);
        assert_eq!(limits.max_orders_per_minute, 5);
    }

    #[test]
    fn build_safety_limits_empty_instrument_is_unrestricted() {
        let limits = build_safety_limits(&SafetyRailsForm::default(), "");
        assert!(limits.allowed_instruments.is_empty());
    }

    #[test]
    fn replay_kpi_handles_no_run() {
        let lr = LastRunResult::default();
        assert!(replay_kpi_summary(&lr).contains("結果なし"));
    }

    #[test]
    fn stepper_clamps_at_zero_and_only_when_open() {
        let mut app = make_app();
        app.add_systems(Update, safety_rails_stepper_system);
        // closed: stepper is ignored.
        app.world_mut().resource_mut::<SafetyRailsForm>().max_daily_loss_jpy = 0;
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SafetyRailsStepper::DailyLossDec));
        app.update();
        assert_eq!(
            app.world().resource::<SafetyRailsForm>().max_daily_loss_jpy,
            0,
            "closed modal must ignore steppers"
        );
        // open: dec clamps at 0, never negative.
        open_prompt(&mut app);
        app.update();
        assert_eq!(
            app.world().resource::<SafetyRailsForm>().max_daily_loss_jpy,
            0,
            "dec must clamp at 0"
        );
    }

    #[test]
    fn stepper_increments_when_open() {
        let mut app = make_app();
        open_prompt(&mut app);
        app.add_systems(Update, safety_rails_stepper_system);
        app.world_mut().resource_mut::<SafetyRailsForm>().max_order_value_jpy = 0;
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SafetyRailsStepper::OrderValueInc,
        ));
        app.update();
        assert_eq!(
            app.world().resource::<SafetyRailsForm>().max_order_value_jpy,
            JPY_STEP
        );
    }

    #[test]
    fn confirm_fires_promote_to_live_and_closes() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        open_prompt(&mut app);
        app.add_systems(Update, safety_rails_modal_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SafetyRailsModalButton::Confirm,
        ));
        app.update();

        assert!(
            app.world().resource::<PromotePrompt>().active.is_none(),
            "Confirm must close the modal"
        );
        let cmd = rx.try_recv().expect("Confirm must fire PromoteToLive");
        match cmd {
            TransportCommand::PromoteToLive {
                strategy_file,
                instrument_id,
                venue,
                safety_limits,
                ensure_live_auto,
                ..
            } => {
                assert_eq!(strategy_file, std::path::PathBuf::from("/tmp/strat.py"));
                assert_eq!(instrument_id, "7203.T");
                assert_eq!(venue, "MOCK");
                assert!(ensure_live_auto, "promote must request LiveAuto");
                assert_eq!(
                    safety_limits.allowed_instruments,
                    vec!["7203.T".to_string()],
                    "whitelist must default to the promoted instrument"
                );
            }
            other => panic!("expected PromoteToLive, got {other:?}"),
        }
    }

    #[test]
    fn cancel_closes_without_firing() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        open_prompt(&mut app);
        app.add_systems(Update, safety_rails_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, SafetyRailsModalButton::Cancel));
        app.update();
        assert!(app.world().resource::<PromotePrompt>().active.is_none());
        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
    }

    #[test]
    fn confirm_is_noop_when_closed() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        // prompt stays closed (default).
        app.add_systems(Update, safety_rails_modal_button_system);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SafetyRailsModalButton::Confirm,
        ));
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "no PromoteToLive may be sent when nothing is pending"
        );
    }

    #[test]
    fn escape_cancels_modal() {
        let mut app = make_app();
        open_prompt(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.add_systems(Update, safety_rails_modal_button_system);
        app.update();
        assert!(
            app.world().resource::<PromotePrompt>().active.is_none(),
            "Esc must close the modal"
        );
    }
}
