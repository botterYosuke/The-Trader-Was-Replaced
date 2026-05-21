//! Phase 9 §3.9 — OrderPanel (LiveManual 専用、手動発注フォーム)。
//!
//! ユーザー選択 (AskUserQuestion 2026-05-20): 本パネルは **Bevy UI Node + Interaction**
//! 流派で実装する (instrument_picker / menu_bar と同じ)。表示専用の world-space sprite
//! floating window (buying_power 等) とは別流派。`PanelKind` / `panel_spawn_dispatcher_system`
//! は経由せず、Startup で 1 度 spawn し `ExecutionMode == LiveManual` のときだけ Display で出す。
//!
//! 2 段階確認: `[発注]` で `OrderConfirm.pending` をセット → 中央オーバーレイの確認モーダルに
//! 内容 (銘柄/売買/数量/価格/概算約定額) を再表示 → `[Confirm]` で初めて
//! `TransportCommand::PlaceOrder` を発射する (§3.9)。
//!
//! 第二暗証番号 (Tachibana) は別モジュール `secret_modal.rs` が `SecretRequired` イベントで
//! 収集する。OrderPanel は `second_secret` を載せない (mock/kabu は不要、Tachibana は Step 5)。

use bevy::prelude::*;

use crate::trading::{
    ExecutionMode, ExecutionModeRes, LastPrices, OrderFeedback, SecretPrompt, SelectedSymbol,
    TransportCommand, TransportCommandSender, VenueStatusRes,
};

// ── デフォルト売買単位・呼値 ───────────────────────────────────────────────
// Phase 9 MVP: 銘柄メタデータ (売買単位 / 呼値) はまだ Rust 側 state に流れていない
// (Tickers は id/name/market のみ)。現物の一般値で代用し、実メタデータ連動は後続
// (account / instrument metadata が流れる Step 4/5) の TODO とする。§3.9。
const DEFAULT_LOT_SIZE: f64 = 100.0;
const DEFAULT_TICK_SIZE: f64 = 1.0;

// ── 配色 ───────────────────────────────────────────────────────────────────
const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.96);
const COLOR_HEADER: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_LABEL: Color = Color::srgb(0.65, 0.70, 0.78);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_ERROR: Color = Color::srgb(1.0, 0.35, 0.45);
const COLOR_BTN_IDLE: Color = Color::srgba(0.18, 0.20, 0.28, 1.0);
const COLOR_BTN_SELECTED: Color = Color::srgba(0.10, 0.40, 0.60, 1.0);
const COLOR_BTN_SUBMIT: Color = Color::srgba(0.10, 0.45, 0.30, 1.0);
const COLOR_BTN_CANCEL: Color = Color::srgba(0.30, 0.16, 0.20, 1.0);
const COLOR_MODAL_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);

// ===========================================================================
// ドメイン型
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    fn wire(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Market,
    Limit,
}

impl OrderType {
    fn wire(self) -> &'static str {
        match self {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
        }
    }
    fn label(self) -> &'static str {
        // 現状 wire と同一だが、確認モーダル表示は `label()` に統一して
        // 将来 wire 文字列が変わっても表示がドリフトしないようにする。
        match self {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Day,
    Opening,
    Closing,
}

impl TimeInForce {
    fn wire(self) -> &'static str {
        match self {
            TimeInForce::Day => "DAY",
            TimeInForce::Opening => "OPENING",
            TimeInForce::Closing => "CLOSING",
        }
    }
    fn label(self) -> &'static str {
        match self {
            TimeInForce::Day => "DAY",
            TimeInForce::Opening => "OPEN",
            TimeInForce::Closing => "CLOSE",
        }
    }
}

/// 発注フォームの現在の入力状態。
#[derive(Resource, Debug, Clone)]
pub struct OrderForm {
    pub side: Side,
    pub order_type: OrderType,
    pub qty: f64,
    pub price: f64,
    pub tif: TimeInForce,
}

impl Default for OrderForm {
    fn default() -> Self {
        Self {
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: DEFAULT_LOT_SIZE,
            price: 0.0,
            tif: TimeInForce::Day,
        }
    }
}

/// 確認モーダルに渡す確定済みドラフト。`build_draft` が `OrderForm` + 選択銘柄 + venue から組む。
#[derive(Debug, Clone, PartialEq)]
pub struct OrderDraft {
    pub venue: String,
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub qty: f64,
    /// 成行は `None`。指値のみ `Some`。
    pub price: Option<f64>,
    pub tif: TimeInForce,
}

/// 2 段階確認の状態。`pending` が `Some` の間だけ確認モーダルを出す。
#[derive(Resource, Default, Debug, Clone)]
pub struct OrderConfirm {
    pub pending: Option<OrderDraft>,
    /// 発注ボタン押下時の検証エラー (パネルに赤字表示)。成功時は `None`。
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderValidationError {
    SymbolNotSelected,
    QtyNotPositive,
    QtyNotLotMultiple,
    PriceRequiredForLimit,
    PriceNotTickMultiple,
}

impl OrderValidationError {
    pub fn message(self) -> &'static str {
        match self {
            OrderValidationError::SymbolNotSelected => "銘柄が未選択です",
            OrderValidationError::QtyNotPositive => "数量は正の値にしてください",
            OrderValidationError::QtyNotLotMultiple => "数量は売買単位の倍数にしてください",
            OrderValidationError::PriceRequiredForLimit => "指値には価格が必要です",
            OrderValidationError::PriceNotTickMultiple => "価格は呼値の倍数にしてください",
        }
    }
}

/// `value` が `step` の (ほぼ) 整数倍かを浮動小数の誤差込みで判定する。
fn is_multiple_of(value: f64, step: f64) -> bool {
    if step <= 0.0 {
        return true;
    }
    let ratio = value / step;
    (ratio - ratio.round()).abs() < 1e-6
}

/// 発注内容を検証する。symbol が無い / 数量が売買単位の倍数でない / 指値なのに価格不正 を弾く。
/// `tick_size` は価格刻みであり数量検証には使わない (§3.9)。
pub fn validate_order(
    form: &OrderForm,
    symbol: Option<&str>,
    lot_size: f64,
    tick_size: f64,
) -> Result<(), OrderValidationError> {
    if symbol.map(|s| s.is_empty()).unwrap_or(true) {
        return Err(OrderValidationError::SymbolNotSelected);
    }
    // Reject non-finite explicitly (NaN slips past `<= 0.0`); mirrors the
    // backend's `math.isfinite` guard (plan Step 2 review) so client/server agree.
    if !form.qty.is_finite() || form.qty <= 0.0 {
        return Err(OrderValidationError::QtyNotPositive);
    }
    if !is_multiple_of(form.qty, lot_size) {
        return Err(OrderValidationError::QtyNotLotMultiple);
    }
    if form.order_type == OrderType::Limit {
        if !form.price.is_finite() || form.price <= 0.0 {
            return Err(OrderValidationError::PriceRequiredForLimit);
        }
        if !is_multiple_of(form.price, tick_size) {
            return Err(OrderValidationError::PriceNotTickMultiple);
        }
    }
    Ok(())
}

/// 検証済みの `OrderForm` を確認用 `OrderDraft` に変換する。成行は price を落とす。
pub fn build_draft(form: &OrderForm, symbol: &str, venue: &str) -> OrderDraft {
    OrderDraft {
        venue: venue.to_string(),
        symbol: symbol.to_string(),
        side: form.side,
        order_type: form.order_type,
        qty: form.qty,
        price: match form.order_type {
            OrderType::Market => None,
            OrderType::Limit => Some(form.price),
        },
        tif: form.tif,
    }
}

/// 概算約定額。指値は draft の価格、成行は直近約定価格 (あれば) で qty を掛ける。
/// 価格が取れない成行は `None`。
pub fn estimated_notional(draft: &OrderDraft, last_price: Option<f64>) -> Option<f64> {
    let unit = draft.price.or(last_price)?;
    Some(unit * draft.qty)
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct OrderPanelRoot;

#[derive(Component, Clone, Copy)]
pub enum OrderButton {
    SideBuy,
    SideSell,
    TypeMarket,
    TypeLimit,
    QtyDec,
    QtyInc,
    PriceDec,
    PriceInc,
    Tif(TimeInForce),
    Submit,
}

#[derive(Component, Clone, Copy)]
pub enum OrderField {
    Symbol,
    Qty,
    Price,
    Error,
}

#[derive(Component)]
pub struct ConfirmModalRoot;

#[derive(Component, Clone, Copy)]
pub enum ConfirmButton {
    Confirm,
    Cancel,
}

#[derive(Component)]
pub struct ConfirmSummary;

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

fn spawn_value_button(parent: &mut ChildBuilder, action: OrderButton, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                margin: UiRect::right(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(COLOR_BTN_IDLE),
            action,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(COLOR_VALUE),
            ));
        });
}

/// ラベル行: 左に固定ラベル、右に子ノード群を並べる横並び Node を作って返す。
fn spawn_row<'a>(parent: &'a mut ChildBuilder, label: &str) -> EntityCommands<'a> {
    let mut row = parent.spawn((Node {
        width: Val::Percent(100.0),
        margin: UiRect::bottom(Val::Px(5.0)),
        align_items: AlignItems::Center,
        ..default()
    },));
    row.with_children(|r| {
        r.spawn((
            Node {
                width: Val::Px(56.0),
                ..default()
            },
            Text::new(label.to_string()),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(COLOR_LABEL),
        ));
    });
    row
}

/// OrderPanel 本体を spawn する (Startup)。初期 Display は None — visibility system が
/// LiveManual のときだけ Flex にする。
pub fn spawn_order_panel(mut commands: Commands) {
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(72.0),
                right: Val::Px(12.0),
                width: Val::Px(244.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(COLOR_PANEL_BG),
            GlobalZIndex(60),
            OrderPanelRoot,
            Name::new("OrderPanel"),
        ))
        .with_children(|p| {
            // ヘッダー
            p.spawn((
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
                Text::new("ORDER (LiveManual)"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(COLOR_HEADER),
            ));

            // 銘柄
            spawn_row(p, "Symbol:").with_children(|r| {
                r.spawn((
                    Text::new("—"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    OrderField::Symbol,
                ));
            });

            // 売買区分
            spawn_row(p, "Side:").with_children(|r| {
                spawn_value_button(r, OrderButton::SideBuy, "BUY");
                spawn_value_button(r, OrderButton::SideSell, "SELL");
            });

            // 注文種別
            spawn_row(p, "Type:").with_children(|r| {
                spawn_value_button(r, OrderButton::TypeMarket, "MKT");
                spawn_value_button(r, OrderButton::TypeLimit, "LIMIT");
            });

            // 数量ステッパー
            spawn_row(p, "Qty:").with_children(|r| {
                spawn_value_button(r, OrderButton::QtyDec, "-");
                r.spawn((
                    Node {
                        width: Val::Px(56.0),
                        margin: UiRect::right(Val::Px(4.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Text::new("100"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    OrderField::Qty,
                ));
                spawn_value_button(r, OrderButton::QtyInc, "+");
            });

            // 価格ステッパー
            spawn_row(p, "Price:").with_children(|r| {
                spawn_value_button(r, OrderButton::PriceDec, "-");
                r.spawn((
                    Node {
                        width: Val::Px(56.0),
                        margin: UiRect::right(Val::Px(4.0)),
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    Text::new("MKT"),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    OrderField::Price,
                ));
                spawn_value_button(r, OrderButton::PriceInc, "+");
            });

            // 執行条件
            spawn_row(p, "TIF:").with_children(|r| {
                spawn_value_button(r, OrderButton::Tif(TimeInForce::Day), "DAY");
                spawn_value_button(r, OrderButton::Tif(TimeInForce::Opening), "OPEN");
                spawn_value_button(r, OrderButton::Tif(TimeInForce::Closing), "CLOSE");
            });

            // エラー行
            p.spawn((
                Node {
                    margin: UiRect::vertical(Val::Px(4.0)),
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(COLOR_ERROR),
                OrderField::Error,
            ));

            // 発注ボタン
            p.spawn((
                Button,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(28.0),
                    margin: UiRect::top(Val::Px(4.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(COLOR_BTN_SUBMIT),
                OrderButton::Submit,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("発注"),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                ));
            });
        });
}

/// 2 段階確認モーダル (中央オーバーレイ) を spawn する (Startup)。初期 Display は None。
pub fn spawn_confirm_modal(mut commands: Commands) {
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
            BackgroundColor(COLOR_MODAL_BACKDROP),
            GlobalZIndex(200),
            ConfirmModalRoot,
            Name::new("OrderConfirmModal"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(320.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(COLOR_PANEL_BG),
            ))
            .with_children(|card| {
                card.spawn((
                    Node {
                        margin: UiRect::bottom(Val::Px(10.0)),
                        ..default()
                    },
                    Text::new("発注内容の確認"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(COLOR_HEADER),
                ));
                // 内容サマリ (sync system が書き換える)
                card.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    ConfirmSummary,
                ));
                // ボタン行
                card.spawn((Node {
                    margin: UiRect::top(Val::Px(14.0)),
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
                            ConfirmButton::Cancel,
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
                            ConfirmButton::Confirm,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("Confirm"),
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

/// OrderPanel root の Display を `ExecutionMode == LiveManual` に同期する。
pub fn order_panel_visibility_system(
    exec_mode: Res<ExecutionModeRes>,
    mut root_q: Query<&mut Node, With<OrderPanelRoot>>,
) {
    let target = if exec_mode.mode == ExecutionMode::LiveManual {
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

/// side/type/TIF/数量±/価格± ボタン押下を `OrderForm` に反映する。
pub fn order_form_button_system(
    interactions: Query<(&Interaction, &OrderButton), (Changed<Interaction>, With<Button>)>,
    mut form: ResMut<OrderForm>,
    mut confirm: ResMut<OrderConfirm>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            OrderButton::SideBuy => form.side = Side::Buy,
            OrderButton::SideSell => form.side = Side::Sell,
            OrderButton::TypeMarket => form.order_type = OrderType::Market,
            OrderButton::TypeLimit => form.order_type = OrderType::Limit,
            OrderButton::QtyDec => {
                form.qty = (form.qty - DEFAULT_LOT_SIZE).max(0.0);
            }
            OrderButton::QtyInc => {
                form.qty += DEFAULT_LOT_SIZE;
            }
            OrderButton::PriceDec => {
                form.price = (form.price - DEFAULT_TICK_SIZE).max(0.0);
            }
            OrderButton::PriceInc => {
                form.price += DEFAULT_TICK_SIZE;
            }
            OrderButton::Tif(tif) => form.tif = *tif,
            // Submit はここでは扱わない (order_submit_button_system)。
            OrderButton::Submit => continue,
        }
        // フォーム編集時は前回の検証エラーを消す。
        confirm.last_error = None;
    }
}

/// `[発注]` 押下で検証 → OK なら `OrderConfirm.pending` をセット (確認モーダルが開く)。
/// NG なら `last_error` にメッセージを入れてパネルに赤字表示する。
pub fn order_submit_button_system(
    interactions: Query<(&Interaction, &OrderButton), (Changed<Interaction>, With<Button>)>,
    form: Res<OrderForm>,
    selected: Res<SelectedSymbol>,
    venue: Res<VenueStatusRes>,
    mut confirm: ResMut<OrderConfirm>,
) {
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if !matches!(button, OrderButton::Submit) {
            continue;
        }
        // 既にモーダルが開いているなら無視 (二重 open 防止)。
        if confirm.pending.is_some() {
            continue;
        }
        let symbol = selected.id.as_deref();
        match validate_order(&form, symbol, DEFAULT_LOT_SIZE, DEFAULT_TICK_SIZE) {
            Ok(()) => {
                let venue_id = venue.venue_id.clone().unwrap_or_default();
                // symbol は validate_order が Some を保証済み。
                let draft = build_draft(&form, symbol.unwrap_or_default(), &venue_id);
                confirm.pending = Some(draft);
                confirm.last_error = None;
            }
            Err(e) => {
                confirm.last_error = Some(e.message().to_string());
            }
        }
    }
}

/// 確認モーダル root の Display を `OrderConfirm.pending` の有無に同期する。
pub fn confirm_modal_visibility_system(
    confirm: Res<OrderConfirm>,
    mut root_q: Query<&mut Node, With<ConfirmModalRoot>>,
) {
    let target = if confirm.pending.is_some() {
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

/// `[Confirm]` → `TransportCommand::PlaceOrder` 発射 + pending クリア。
/// `[Cancel]` → pending クリア (発注しない)。
pub fn confirm_modal_button_system(
    interactions: Query<(&Interaction, &ConfirmButton), (Changed<Interaction>, With<Button>)>,
    keys: Res<ButtonInput<KeyCode>>,
    secret_prompt: Res<SecretPrompt>,
    mut confirm: ResMut<OrderConfirm>,
    mut feedback: ResMut<OrderFeedback>,
    sender: Option<Res<TransportCommandSender>>,
) {
    // Item 8: this is the single most safety-critical button (real-money
    // PlaceOrder). Guard on open-state — never act on a stray `Pressed` for a
    // `ConfirmButton` when no order is pending (mirrors modify/context-menu
    // systems; the Display::None zero-size invariant is the only other latch).
    if confirm.pending.is_none() {
        return;
    }

    // Item 9: Esc cancels the confirm modal (clears pending, fires nothing),
    // consistent with every other Phase 9 modal. Escape is read via ButtonInput
    // (not the SecretModal event drain), so yield to an open SecretModal so one
    // keystroke can't close both (§3.10 / item 7 prioritization). The confirm
    // modal is otherwise high priority — it does NOT yield to notice modals.
    if keys.just_pressed(KeyCode::Escape) && secret_prompt.active.is_none() {
        confirm.pending = None;
        return;
    }

    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            ConfirmButton::Cancel => {
                confirm.pending = None;
            }
            ConfirmButton::Confirm => {
                let Some(draft) = confirm.pending.take() else {
                    continue;
                };
                // Fresh attempt: clear any stale reject/timeout notice.
                feedback.message = None;
                if let Some(tx) = sender.as_ref() {
                    let _ = tx.tx.send(TransportCommand::PlaceOrder {
                        venue: draft.venue,
                        instrument_id: draft.symbol,
                        side: draft.side.wire().to_string(),
                        qty: draft.qty,
                        price: draft.price,
                        order_type: draft.order_type.wire().to_string(),
                        time_in_force: draft.tif.wire().to_string(),
                        // Tachibana 第二暗証番号は secret_modal が SecretRequired で別途収集 (Step 5)。
                        second_secret: None,
                    });
                } else {
                    warn!("PlaceOrder skipped: TransportCommandSender unavailable");
                }
            }
        }
    }
}

/// OrderForm / 選択銘柄をパネルの値テキストと選択ボタン色に差分反映する。
pub fn order_panel_sync_system(
    form: Res<OrderForm>,
    selected: Res<SelectedSymbol>,
    confirm: Res<OrderConfirm>,
    feedback: Res<OrderFeedback>,
    mut fields: Query<(&OrderField, &mut Text)>,
    mut buttons: Query<(&OrderButton, &mut BackgroundColor), With<Button>>,
) {
    // 値テキスト
    for (field, mut text) in &mut fields {
        let new = match field {
            OrderField::Symbol => selected.id.clone().unwrap_or_else(|| "—".to_string()),
            OrderField::Qty => format!("{:.0}", form.qty),
            OrderField::Price => match form.order_type {
                OrderType::Market => "MKT".to_string(),
                OrderType::Limit => format!("{:.0}", form.price),
            },
            // 検証エラーを優先、無ければ RPC reject / secret timeout の通知を出す。
            OrderField::Error => confirm
                .last_error
                .clone()
                .or_else(|| feedback.message.clone())
                .unwrap_or_default(),
        };
        if text.0 != new {
            text.0 = new;
        }
    }

    // 選択中ボタンをハイライト
    for (button, mut bg) in &mut buttons {
        let selected_now = match button {
            OrderButton::SideBuy => form.side == Side::Buy,
            OrderButton::SideSell => form.side == Side::Sell,
            OrderButton::TypeMarket => form.order_type == OrderType::Market,
            OrderButton::TypeLimit => form.order_type == OrderType::Limit,
            OrderButton::Tif(tif) => form.tif == *tif,
            _ => continue,
        };
        let target = if selected_now {
            COLOR_BTN_SELECTED
        } else {
            COLOR_BTN_IDLE
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

/// 確認モーダルのサマリテキストを `pending` ドラフトから差分反映する。
pub fn confirm_modal_sync_system(
    confirm: Res<OrderConfirm>,
    last_prices: Res<LastPrices>,
    mut summary_q: Query<&mut Text, With<ConfirmSummary>>,
) {
    let Some(draft) = confirm.pending.as_ref() else {
        return;
    };
    let Ok(mut text) = summary_q.get_single_mut() else {
        return;
    };
    let price_str = match draft.price {
        Some(p) => format!("{p:.0}"),
        None => "成行".to_string(),
    };
    let last = last_prices.map.get(&draft.symbol).copied();
    let notional = estimated_notional(draft, last)
        .map(|n| format!("{n:.0}"))
        .unwrap_or_else(|| "—".to_string());
    let new = format!(
        "venue: {}\n銘柄: {}\n売買: {}\n種別: {}\n数量: {:.0}\n価格: {}\n執行: {}\n概算約定額: {} (手数料概算は未対応)",
        draft.venue,
        draft.symbol,
        draft.side.label(),
        draft.order_type.label(),
        draft.qty,
        price_str,
        draft.tif.label(),
        notional,
    );
    if text.0 != new {
        text.0 = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(order_type: OrderType, qty: f64, price: f64) -> OrderForm {
        OrderForm {
            side: Side::Buy,
            order_type,
            qty,
            price,
            tif: TimeInForce::Day,
        }
    }

    #[test]
    fn validate_rejects_missing_symbol() {
        let f = form(OrderType::Market, 100.0, 0.0);
        assert_eq!(
            validate_order(&f, None, 100.0, 1.0),
            Err(OrderValidationError::SymbolNotSelected)
        );
        assert_eq!(
            validate_order(&f, Some(""), 100.0, 1.0),
            Err(OrderValidationError::SymbolNotSelected)
        );
    }

    #[test]
    fn validate_rejects_non_positive_qty() {
        let f = form(OrderType::Market, 0.0, 0.0);
        assert_eq!(
            validate_order(&f, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::QtyNotPositive)
        );
    }

    #[test]
    fn validate_rejects_non_lot_multiple_qty() {
        let f = form(OrderType::Market, 150.0, 0.0);
        assert_eq!(
            validate_order(&f, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::QtyNotLotMultiple)
        );
    }

    #[test]
    fn validate_market_order_ignores_price() {
        // 成行は price=0 でも通る (price 検証は指値のみ)。
        let f = form(OrderType::Market, 100.0, 0.0);
        assert_eq!(validate_order(&f, Some("7203.T"), 100.0, 1.0), Ok(()));
    }

    #[test]
    fn validate_limit_requires_price() {
        let f = form(OrderType::Limit, 100.0, 0.0);
        assert_eq!(
            validate_order(&f, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::PriceRequiredForLimit)
        );
    }

    #[test]
    fn validate_limit_rejects_non_tick_price() {
        let f = form(OrderType::Limit, 100.0, 2500.5);
        assert_eq!(
            validate_order(&f, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::PriceNotTickMultiple)
        );
    }

    #[test]
    fn validate_limit_accepts_tick_price() {
        let f = form(OrderType::Limit, 100.0, 2500.0);
        assert_eq!(validate_order(&f, Some("7203.T"), 100.0, 1.0), Ok(()));
    }

    #[test]
    fn validate_rejects_non_finite_qty_and_price() {
        let nan_qty = form(OrderType::Market, f64::NAN, 0.0);
        assert_eq!(
            validate_order(&nan_qty, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::QtyNotPositive)
        );
        let inf_qty = form(OrderType::Market, f64::INFINITY, 0.0);
        assert_eq!(
            validate_order(&inf_qty, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::QtyNotPositive)
        );
        let nan_price = form(OrderType::Limit, 100.0, f64::NAN);
        assert_eq!(
            validate_order(&nan_price, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::PriceRequiredForLimit)
        );
        // Inf price is caught by the is_finite guard with the right message
        // (not the tick-multiple fallthrough).
        let inf_price = form(OrderType::Limit, 100.0, f64::INFINITY);
        assert_eq!(
            validate_order(&inf_price, Some("7203.T"), 100.0, 1.0),
            Err(OrderValidationError::PriceRequiredForLimit)
        );
    }

    #[test]
    fn build_draft_drops_price_for_market() {
        let f = form(OrderType::Market, 100.0, 2500.0);
        let d = build_draft(&f, "7203.T", "MOCK");
        assert_eq!(d.price, None, "成行は price を載せない");
        assert_eq!(d.symbol, "7203.T");
        assert_eq!(d.venue, "MOCK");
    }

    #[test]
    fn build_draft_keeps_price_for_limit() {
        let f = form(OrderType::Limit, 200.0, 2500.0);
        let d = build_draft(&f, "7203.T", "kabu");
        assert_eq!(d.price, Some(2500.0));
        assert_eq!(d.qty, 200.0);
    }

    #[test]
    fn notional_uses_limit_price() {
        let f = form(OrderType::Limit, 100.0, 2500.0);
        let d = build_draft(&f, "7203.T", "MOCK");
        assert_eq!(estimated_notional(&d, None), Some(250000.0));
    }

    #[test]
    fn notional_falls_back_to_last_price_for_market() {
        let f = form(OrderType::Market, 100.0, 0.0);
        let d = build_draft(&f, "7203.T", "MOCK");
        assert_eq!(estimated_notional(&d, Some(2400.0)), Some(240000.0));
        assert_eq!(
            estimated_notional(&d, None),
            None,
            "価格不明の成行は概算不可"
        );
    }

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<OrderForm>();
        app.init_resource::<OrderConfirm>();
        app.init_resource::<OrderFeedback>();
        app.init_resource::<SecretPrompt>();
        app.init_resource::<ButtonInput<KeyCode>>();
        app.insert_resource(SelectedSymbol {
            id: Some("7203.T".to_string()),
        });
        app.insert_resource(VenueStatusRes {
            venue_id: Some("MOCK".to_string()),
            ..Default::default()
        });
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().spawn(RxHolder { _rx: rx });
        app
    }

    // テスト中に receiver を生かしておくための holder。
    #[derive(Component)]
    struct RxHolder {
        _rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    }

    #[test]
    fn submit_sets_pending_when_valid() {
        let mut app = make_app();
        app.add_systems(Update, order_submit_button_system);
        let btn = app
            .world_mut()
            .spawn((Button, Interaction::Pressed, OrderButton::Submit))
            .id();
        app.update();
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "valid submit must open the confirm modal"
        );
        let _ = btn;
    }

    #[test]
    fn submit_sets_error_when_symbol_missing() {
        let mut app = make_app();
        app.world_mut().resource_mut::<SelectedSymbol>().id = None;
        app.add_systems(Update, order_submit_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, OrderButton::Submit));
        app.update();
        let confirm = app.world().resource::<OrderConfirm>();
        assert!(
            confirm.pending.is_none(),
            "invalid submit must not open modal"
        );
        assert!(
            confirm.last_error.is_some(),
            "invalid submit must set an error"
        );
    }

    #[test]
    fn confirm_fires_place_order_and_clears_pending() {
        let mut app = make_app();
        // 受信側を保持して送信を観測する。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });
        app.add_systems(Update, confirm_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Confirm));
        app.update();

        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "Confirm must clear pending"
        );
        let cmd = rx
            .try_recv()
            .expect("Confirm must fire a PlaceOrder command");
        match cmd {
            TransportCommand::PlaceOrder {
                venue,
                instrument_id,
                side,
                qty,
                price,
                order_type,
                second_secret,
                ..
            } => {
                assert_eq!(venue, "MOCK");
                assert_eq!(instrument_id, "7203.T");
                assert_eq!(side, "BUY");
                assert_eq!(qty, 100.0);
                assert_eq!(price, None);
                assert_eq!(order_type, "MARKET");
                assert!(
                    second_secret.is_none(),
                    "OrderPanel never carries the secret"
                );
            }
            other => panic!("expected PlaceOrder, got {other:?}"),
        }
    }

    #[test]
    fn cancel_clears_pending_without_firing() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });
        app.add_systems(Update, confirm_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Cancel));
        app.update();

        assert!(app.world().resource::<OrderConfirm>().pending.is_none());
        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
    }

    /// Item 8 regression: with NO order pending, a stray `ConfirmButton::Confirm`
    /// Pressed must NOT fire a PlaceOrder (the single most safety-critical button).
    #[test]
    fn confirm_button_is_noop_when_pending_none() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        // pending stays None (default).
        app.add_systems(Update, confirm_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ConfirmButton::Confirm));
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "no PlaceOrder may be sent when nothing is pending"
        );
        assert!(app.world().resource::<OrderConfirm>().pending.is_none());
    }

    /// Item 9 regression: Esc cancels the confirm modal — clears pending, fires
    /// nothing — consistent with the other Phase 9 modals.
    #[test]
    fn escape_cancels_confirm_modal() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.add_systems(Update, confirm_modal_button_system);
        app.update();
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_none(),
            "Esc must clear pending (cancel)"
        );
        assert!(rx.try_recv().is_err(), "Esc must not fire a command");
    }

    /// Item 9 + item 7: while a SecretModal is open, Esc is consumed by the secret
    /// modal — the confirm modal must NOT also close on the same keystroke.
    #[test]
    fn escape_on_confirm_yields_to_open_secret_prompt() {
        use crate::trading::SecretPromptRequest;
        let mut app = make_app();
        app.world_mut().resource_mut::<OrderConfirm>().pending = Some(OrderDraft {
            venue: "MOCK".to_string(),
            symbol: "7203.T".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            qty: 100.0,
            price: None,
            tif: TimeInForce::Day,
        });
        app.world_mut().resource_mut::<SecretPrompt>().active = Some(SecretPromptRequest {
            request_id: "r1".to_string(),
            venue: "MOCK".to_string(),
            kind: "second_password".to_string(),
            purpose: "new_order".to_string(),
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.add_systems(Update, confirm_modal_button_system);
        app.update();
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "confirm modal must survive Escape consumed by the SecretModal"
        );
    }

    #[test]
    fn form_buttons_mutate_state() {
        let mut app = make_app();
        app.add_systems(Update, order_form_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, OrderButton::SideSell));
        app.world_mut()
            .spawn((Button, Interaction::Pressed, OrderButton::TypeLimit));
        app.world_mut()
            .spawn((Button, Interaction::Pressed, OrderButton::QtyInc));
        app.update();
        let f = app.world().resource::<OrderForm>();
        assert_eq!(f.side, Side::Sell);
        assert_eq!(f.order_type, OrderType::Limit);
        assert_eq!(f.qty, 200.0, "QtyInc adds one lot");
    }
}
