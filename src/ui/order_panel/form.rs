//! OrderPanel フォーム本体 (Phase 9 §3.9)。order_panel/mod.rs から分割。
//! ドメイン型・検証・ドラフト生成・floating-window コンテンツ・フォーム系 systems。

use bevy::prelude::*;

use crate::trading::{
    ExecutionMode, ExecutionModeRes, OrderFeedback, SelectedSymbol, VenueStatusRes,
};
use crate::ui::components::{PanelKind, WindowRoot};

use super::confirm_modal::OrderConfirm;

// ── デフォルト売買単位・呼値 ───────────────────────────────────────────────
// Phase 9 MVP: 銘柄メタデータ (売買単位 / 呼値) はまだ Rust 側 state に流れていない
// (Tickers は id/name/market のみ)。現物の一般値で代用し、実メタデータ連動は後続
// (account / instrument metadata が流れる Step 4/5) の TODO とする。§3.9。
const DEFAULT_LOT_SIZE: f64 = 100.0;
const DEFAULT_TICK_SIZE: f64 = 1.0;

// Colors sourced from Theme (see spawn functions below)

// ===========================================================================
// ドメイン型
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub(super) fn wire(self) -> &'static str {
        match self {
            Side::Buy => "BUY",
            Side::Sell => "SELL",
        }
    }
    pub(super) fn label(self) -> &'static str {
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
    pub(super) fn wire(self) -> &'static str {
        match self {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
        }
    }
    pub(super) fn label(self) -> &'static str {
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
    pub(super) fn wire(self) -> &'static str {
        match self {
            TimeInForce::Day => "DAY",
            TimeInForce::Opening => "OPENING",
            TimeInForce::Closing => "CLOSING",
        }
    }
    pub(super) fn label(self) -> &'static str {
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
// Components + Events
// ===========================================================================

/// ボタン操作を world-space observer から systems に橋渡しするイベント。
#[derive(Message, Debug, Clone, Copy)]
pub struct OrderButtonPressed(pub OrderButton);

#[derive(Component, Clone, Copy, Debug)]
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

// ===========================================================================
// Floating-window content
// ===========================================================================

pub fn spawn_order_form_in_window(commands: &mut Commands, content_area: Entity) {
    let theme = crate::ui::theme::Theme::default();
    let color_label = theme.colors.text_muted;
    let color_error = theme.status.error;
    let color_btn_idle = theme.colors.element_background;
    let color_btn_selected = theme.colors.element_selected;
    const LABEL_X: f32 = -95.0;
    const Y_SYMBOL: f32 = 128.0;
    const Y_SIDE: f32 = 98.0;
    const Y_TYPE: f32 = 68.0;
    const Y_QTY: f32 = 38.0;
    const Y_PRICE: f32 = 8.0;
    const Y_TIF: f32 = -22.0;
    const Y_ERROR: f32 = -58.0;
    const Y_SUBMIT: f32 = -100.0;
    const BTN_H: f32 = 22.0;
    const BTN_SM_W: f32 = 64.0;
    const BTN_STEP_W: f32 = 28.0;
    const BTN_TIF_W: f32 = 54.0;
    const BTN_SUBMIT_W: f32 = 252.0;
    const BTN_SUBMIT_H: f32 = 28.0;
    const FONT_SZ: f32 = 11.0;

    let mut spawn_label = |commands: &mut Commands, text: &str, x: f32, y: f32| {
        commands.entity(content_area).with_children(|p| {
            p.spawn((
                Text2d::new(text.to_string()),
                TextFont { font_size: FONT_SZ, ..default() },
                TextColor(color_label),
                Transform::from_xyz(x, y, 0.1),
                bevy::sprite::Anchor::CENTER_RIGHT,
            ));
        });
    };

    let mut spawn_field = |commands: &mut Commands, field: OrderField, text: &str, x: f32, y: f32| {
        commands.entity(content_area).with_children(|p| {
            p.spawn((
                Text2d::new(text.to_string()),
                TextFont { font_size: FONT_SZ, ..default() },
                TextColor(theme.colors.text),
                Transform::from_xyz(x, y, 0.1),
                field,
            ));
        });
    };

    let mut spawn_btn = |commands: &mut Commands, action: OrderButton, label: &str, x: f32, y: f32, w: f32, h: f32, color: Color| {
        let btn = action;
        commands.entity(content_area).with_children(|p| {
            p.spawn((
                Sprite { color, custom_size: Some(Vec2::new(w, h)), ..default() },
                Transform::from_xyz(x, y, 0.2),
                btn,
            ))
            .observe(move |_trigger: On<Pointer<Click>>, mut ev: MessageWriter<OrderButtonPressed>| {
                ev.write(OrderButtonPressed(btn));
            })
            .with_children(|s| {
                s.spawn((
                    Text2d::new(label.to_string()),
                    TextFont { font_size: FONT_SZ, ..default() },
                    TextColor(theme.colors.text),
                    Transform::from_xyz(0.0, 0.0, 0.1),
                ));
            });
        });
    };

    spawn_label(commands, "Symbol", LABEL_X, Y_SYMBOL);
    spawn_field(commands, OrderField::Symbol, "----", 30.0, Y_SYMBOL);

    spawn_label(commands, "Side", LABEL_X, Y_SIDE);
    spawn_btn(commands, OrderButton::SideBuy, "BUY", -50.0, Y_SIDE, BTN_SM_W, BTN_H, color_btn_selected);
    spawn_btn(commands, OrderButton::SideSell, "SELL", 22.0, Y_SIDE, BTN_SM_W, BTN_H, color_btn_idle);

    spawn_label(commands, "Type", LABEL_X, Y_TYPE);
    spawn_btn(commands, OrderButton::TypeMarket, "MKT", -50.0, Y_TYPE, BTN_SM_W, BTN_H, color_btn_selected);
    spawn_btn(commands, OrderButton::TypeLimit, "LIMIT", 22.0, Y_TYPE, BTN_SM_W, BTN_H, color_btn_idle);

    spawn_label(commands, "Qty", LABEL_X, Y_QTY);
    spawn_btn(commands, OrderButton::QtyDec, "-", -78.0, Y_QTY, BTN_STEP_W, BTN_H, color_btn_idle);
    spawn_field(commands, OrderField::Qty, "100", -26.0, Y_QTY);
    spawn_btn(commands, OrderButton::QtyInc, "+", 25.0, Y_QTY, BTN_STEP_W, BTN_H, color_btn_idle);

    spawn_label(commands, "Price", LABEL_X, Y_PRICE);
    spawn_btn(commands, OrderButton::PriceDec, "-", -78.0, Y_PRICE, BTN_STEP_W, BTN_H, color_btn_idle);
    spawn_field(commands, OrderField::Price, "----", -26.0, Y_PRICE);
    spawn_btn(commands, OrderButton::PriceInc, "+", 25.0, Y_PRICE, BTN_STEP_W, BTN_H, color_btn_idle);

    spawn_label(commands, "TIF", LABEL_X, Y_TIF);
    spawn_btn(commands, OrderButton::Tif(TimeInForce::Day), "DAY", -72.0, Y_TIF, BTN_TIF_W, BTN_H, color_btn_selected);
    spawn_btn(commands, OrderButton::Tif(TimeInForce::Opening), "OPEN", -12.0, Y_TIF, BTN_TIF_W, BTN_H, color_btn_idle);
    spawn_btn(commands, OrderButton::Tif(TimeInForce::Closing), "CLOSE", 48.0, Y_TIF, BTN_TIF_W, BTN_H, color_btn_idle);

    commands.entity(content_area).with_children(|p| {
        p.spawn((
            Text2d::new(""),
            TextFont { font_size: FONT_SZ, ..default() },
            TextColor(color_error),
            Transform::from_xyz(0.0, Y_ERROR, 0.1),
            OrderField::Error,
        ));
    });

    let submit = OrderButton::Submit;
    commands.entity(content_area).with_children(|p| {
        p.spawn((
            Sprite { color: theme.status.success_background, custom_size: Some(Vec2::new(BTN_SUBMIT_W, BTN_SUBMIT_H)), ..default() },
            Transform::from_xyz(0.0, Y_SUBMIT, 0.2),
            submit,
        ))
        .observe(move |_trigger: On<Pointer<Click>>, mut ev: MessageWriter<OrderButtonPressed>| {
            ev.write(OrderButtonPressed(submit));
        })
        .with_children(|s| {
            s.spawn((
                Text2d::new("発注"),
                TextFont { font_size: FONT_SZ + 2.0, ..default() },
                TextColor(theme.colors.text),
                Transform::from_xyz(0.0, 0.0, 0.1),
            ));
        });
    });
}

// ===========================================================================
// Systems
// ===========================================================================

/// side/type/TIF/数量±/価格± ボタン押下を `OrderForm` に反映する。
pub fn order_form_button_system(
    mut events: MessageReader<OrderButtonPressed>,
    mut form: ResMut<OrderForm>,
    mut confirm: ResMut<OrderConfirm>,
) {
    for OrderButtonPressed(button) in events.read() {
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
    mut events: MessageReader<OrderButtonPressed>,
    form: Res<OrderForm>,
    selected: Res<SelectedSymbol>,
    venue: Res<VenueStatusRes>,
    mut confirm: ResMut<OrderConfirm>,
) {
    for OrderButtonPressed(button) in events.read() {
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

/// ExecutionMode が LiveManual を外れたとき、ORDER floating window をすべて despawn する。
pub fn order_window_despawn_system(
    exec_mode: Res<ExecutionModeRes>,
    panel_q: Query<(Entity, &PanelKind), With<WindowRoot>>,
    mut commands: Commands,
) {
    if !exec_mode.is_changed() {
        return;
    }
    if exec_mode.mode == ExecutionMode::LiveManual {
        return;
    }
    for (entity, kind) in &panel_q {
        if matches!(kind, PanelKind::Order) {
            commands.entity(entity).despawn();
        }
    }
}

/// OrderForm / 選択銘柄をパネルの値テキストと選択ボタン色に差分反映する。
/// Slice 2 以降: ORDER window は world-space sprite のため Text2d / Sprite を使う。
pub fn order_panel_sync_system(
    form: Res<OrderForm>,
    selected: Res<SelectedSymbol>,
    confirm: Res<OrderConfirm>,
    feedback: Res<OrderFeedback>,
    theme: Res<crate::ui::theme::Theme>,
    mut fields: Query<(&OrderField, &mut Text2d, &mut TextColor)>,
    mut buttons: Query<(&OrderButton, &mut Sprite)>,
) {
    // 値テキスト
    for (field, mut text, _color) in &mut fields {
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
    for (button, mut sprite) in &mut buttons {
        let selected_now = match button {
            OrderButton::SideBuy => form.side == Side::Buy,
            OrderButton::SideSell => form.side == Side::Sell,
            OrderButton::TypeMarket => form.order_type == OrderType::Market,
            OrderButton::TypeLimit => form.order_type == OrderType::Limit,
            OrderButton::Tif(tif) => form.tif == *tif,
            _ => continue,
        };
        let target = if selected_now {
            theme.colors.element_selected
        } else {
            theme.colors.element_background
        };
        if sprite.color != target {
            sprite.color = target;
        }
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

    #[test]
    fn form_buttons_mutate_state() {
        let mut app = App::new();
        app.add_message::<OrderButtonPressed>();
        app.init_resource::<OrderForm>();
        app.init_resource::<OrderConfirm>();
        app.add_systems(Update, order_form_button_system);
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::SideSell));
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::TypeLimit));
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::QtyInc));
        app.update();
        let f = app.world().resource::<OrderForm>();
        assert_eq!(f.side, Side::Sell);
        assert_eq!(f.order_type, OrderType::Limit);
        assert_eq!(f.qty, 200.0, "QtyInc adds one lot");
    }

    // ── issue #25 Slice 2: ORDER window の LiveManual 離脱 despawn ──────────────
    fn count_order_kind(app: &mut App) -> usize {
        let mut q = app.world_mut().query::<&PanelKind>();
        q.iter(app.world())
            .filter(|k| matches!(k, PanelKind::Order))
            .count()
    }

    #[test]
    fn order_window_despawns_when_leaving_live_manual() {
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.add_systems(Update, order_window_despawn_system);

        // ORDER floating window root（WindowRoot + PanelKind::Order）。
        let window = app.world_mut().spawn((WindowRoot, PanelKind::Order)).id();
        // サイドバーの Order ボタンも PanelKind::Order を marker に持つ（sidebar.rs）。
        // despawn は WINDOW だけを対象にし、ボタンは Visibility で gate されるため残す。
        let button = app.world_mut().spawn((Button, PanelKind::Order)).id();

        // mode は LiveManual 外（Replay）かつ resource は今 insert したので is_changed。
        app.update();

        assert!(
            app.world().get_entity(window).is_err(),
            "leaving LiveManual must despawn the ORDER floating window"
        );
        assert!(
            app.world().get_entity(button).is_ok(),
            "the sidebar Order button (no WindowRoot) must survive — it is hidden via Visibility, not despawned"
        );
    }

    #[test]
    fn order_window_survives_inside_live_manual() {
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.add_systems(Update, order_window_despawn_system);

        let window = app.world_mut().spawn((WindowRoot, PanelKind::Order)).id();
        app.update();

        assert!(
            app.world().get_entity(window).is_ok(),
            "ORDER window must persist while in LiveManual"
        );
    }

    #[test]
    fn order_window_despawn_is_gated_on_mode_change() {
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.add_systems(Update, order_window_despawn_system);

        // 1 回目: insert 直後なので is_changed → 既存 window を despawn。
        let first = app.world_mut().spawn((WindowRoot, PanelKind::Order)).id();
        app.update();
        assert!(app.world().get_entity(first).is_err());

        // 2 回目: mode を触らずに新しい window を spawn。is_changed=false なので
        // 毎フレーム despawn せず、生き残る（spurious despawn 防止の不変条件）。
        let second = app.world_mut().spawn((WindowRoot, PanelKind::Order)).id();
        app.update();
        assert!(
            app.world().get_entity(second).is_ok(),
            "without a mode change the system must not despawn windows every frame"
        );
        assert_eq!(count_order_kind(&mut app), 1, "only the second window remains");
    }
}
