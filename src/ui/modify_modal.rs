//! Phase 9 §3.11 / §3.12 (Step 4) — Modify (訂正) モーダル。
//!
//! OrdersPanel の右クリックコンテキストメニュー `[訂正]` から開く中央オーバーレイ。
//! `order_panel.rs` / `secret_modal.rs` と同じ **Bevy UI Node + Interaction** 流派で、
//! 数量 / 価格の入力は keyboard イベント drain (picker_searchbox / secret_modal と同じ手法)。
//! 空欄は「変更しない (None)」扱い。
//!
//! kabu 警告バナー (§2.3 / §3.11): 対象 venue が kabu (= `supports_order_correction == false`)
//! のとき、上部に「取消→新規発注の 2 段階で訂正、途中失敗で元注文のみ取消の恐れ」警告を出し、
//! `[理解した上で訂正する]` チェックを ON にするまで `[Confirm]` を disabled にする。
//! Tachibana / mock(MOCK) は警告不要・チェック不要 (atomic な CLMKabuCorrectOrder)。
//!
//! `[Confirm]` で `TransportCommand::ModifyOrder { venue, client_order_id, new_qty, new_price,
//! second_secret: None }` を発射してモーダルを閉じる。`[Cancel]` / Esc は破棄。
//! `OrderEvent` は qty/price を運ばないため、qty/price は本コマンド由来の値をマージ側
//! (transport task → `OrderModified`) で使う (§3.2)。

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;

use crate::trading::{OrderFeedback, TransportCommand, TransportCommandSender};
use crate::venue_capabilities::for_venue;
use crate::ui::component::modal_layer::{
    DismissDecision, ModalHandle, ModalLayer, ModalSkeleton, reconcile_modal_stack, spawn_modal,
};
use crate::ui::theme::{LabelSize, Theme};

const COLOR_FIELD_BG: Color = Color::srgba(0.04, 0.04, 0.08, 1.0);
const COLOR_FIELD_BG_ACTIVE: Color = Color::srgba(0.10, 0.14, 0.22, 1.0);
// Confirm button initial bg (starts disabled); active/disabled color is owned
// by button_interaction_system (Tinted(Success) + ButtonDisabled). #46 Slice A.
const COLOR_BTN_DISABLED: Color = Color::srgba(0.18, 0.20, 0.24, 1.0);
const COLOR_BTN_CANCEL: Color = Color::srgba(0.30, 0.16, 0.20, 1.0);
const COLOR_CHECK_OFF: Color = Color::srgba(0.18, 0.20, 0.28, 1.0);
const COLOR_CHECK_ON: Color = Color::srgba(0.10, 0.45, 0.30, 1.0);

const KABU_WARNING: &str = "kabuステーションには訂正 API がありません。取消→新規発注の 2 段階で訂正します。途中失敗で元注文のみ取消になることがあります。";

// ===========================================================================
// Resource / domain
// ===========================================================================

/// Modify モーダルの状態。`open` が true の間だけモーダルを出す。`new_qty_buf` /
/// `new_price_buf` は keyboard drain の入力先 (空欄=変更なし)。`ack_kabu` は kabu 警告の
/// 同意チェック。`venue` が kabu のときだけ `ack_kabu` が Confirm の前提になる。
#[derive(Resource, Default, Debug, Clone)]
pub struct ModifyForm {
    pub open: bool,
    pub client_order_id: String,
    pub venue: String,
    pub new_qty_buf: String,
    pub new_price_buf: String,
    pub ack_kabu: bool,
    /// どちらの入力欄にフォーカスがあるか。keyboard drain の宛先。
    pub focus: ModifyFocus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModifyFocus {
    #[default]
    Qty,
    Price,
}

impl ModifyForm {
    /// 対象 venue が kabu (= 訂正 API なし) なら true。Tachibana / mock は false。
    pub fn requires_kabu_ack(&self) -> bool {
        for_venue(&self.venue)
            .map(|c| !c.supports_order_correction)
            .unwrap_or(false)
    }

    /// 入力バッファをパースした `(new_qty, new_price)`。空欄 / パース不能は `None`。
    pub fn parsed(&self) -> (Option<f64>, Option<f64>) {
        (parse_buf(&self.new_qty_buf), parse_buf(&self.new_price_buf))
    }

    /// Confirm 可能か。① qty/price の少なくとも一方が有効な変更、かつ ② kabu なら ack 済み。
    pub fn can_confirm(&self) -> bool {
        let (q, p) = self.parsed();
        let has_change = q.is_some() || p.is_some();
        let kabu_ok = !self.requires_kabu_ack() || self.ack_kabu;
        has_change && kabu_ok
    }

    fn close(&mut self) {
        self.open = false;
        self.client_order_id.clear();
        self.venue.clear();
        self.new_qty_buf.clear();
        self.new_price_buf.clear();
        self.ack_kabu = false;
        self.focus = ModifyFocus::Qty;
    }
}

/// 入力バッファを `Option<f64>` にパースする。空欄・空白のみ・非有限・<=0 は `None`。
fn parse_buf(buf: &str) -> Option<f64> {
    let t = buf.trim();
    if t.is_empty() {
        return None;
    }
    match t.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => Some(v),
        _ => None,
    }
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct ModifyModalRoot;

#[derive(Component)]
pub struct ModifyTitleText;

#[derive(Component)]
pub struct ModifyWarnRow;

/// チェックボックス行のマーカー (visibility 制御用)。
#[derive(Component)]
pub struct ModifyWarnAckRow;

#[derive(Component, Clone, Copy)]
pub enum ModifyField {
    Qty,
    Price,
}

#[derive(Component, Clone, Copy)]
pub struct ModifyFieldBg(pub ModifyFocus);

#[derive(Component)]
pub struct ModifyAckCheckbox;

#[derive(Component)]
pub struct ModifyAckText;

#[derive(Component, Clone, Copy)]
pub enum ModifyButton {
    Confirm,
    Cancel,
    AckToggle,
    FocusQty,
    FocusPrice,
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

pub fn spawn_modify_modal(mut commands: Commands, theme: Res<Theme>) {
    let ModalHandle { root, card } = spawn_modal(
        &mut commands,
        &theme,
        ModalSkeleton {
            width: 340.0,
            // 確認モーダル (200) より前面、secret modal (300) より背面。
            z_index: 250,
            name: "ModifyModal",
        },
    );

    commands.entity(root).insert(ModifyModalRoot);

    let header = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(8.0)),
                ..default()
            },
            Text::new("注文の訂正"),
            theme.typography.label_font(LabelSize::Large),
            TextColor(theme.colors.text_accent),
            ModifyTitleText,
        ))
        .id();

    // kabu 警告バナー (Display で出し入れ、初期は None)。
    let warn_text = commands
        .spawn((
            Text::new(KABU_WARNING),
            theme.typography.label_font(LabelSize::Small),
            TextColor(theme.status.warning),
        ))
        .id();
    let warn_row = commands
        .spawn((
            Node {
                display: Display::None,
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(8.0)),
                margin: UiRect::bottom(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(theme.status.warning_background),
            ModifyWarnRow,
        ))
        .add_child(warn_text)
        .id();

    let qty_row = spawn_input_row(
        &mut commands,
        &theme,
        "新数量:",
        ModifyField::Qty,
        ModifyButton::FocusQty,
    );
    let price_row = spawn_input_row(
        &mut commands,
        &theme,
        "新価格:",
        ModifyField::Price,
        ModifyButton::FocusPrice,
    );

    // kabu 同意チェックボックス行 (kabu のときだけ可視: sync system が制御、初期 None)。
    let checkbox = commands
        .spawn((
            Node {
                width: Val::Px(16.0),
                height: Val::Px(16.0),
                ..default()
            },
            BackgroundColor(COLOR_CHECK_OFF),
            ModifyAckCheckbox,
        ))
        .id();
    let ack_text = commands
        .spawn((
            Text::new("理解した上で訂正する"),
            theme.typography.label_font(LabelSize::Small),
            TextColor(theme.colors.text),
            ModifyAckText,
        ))
        .id();
    let ack_row = commands
        .spawn((
            Button,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                margin: UiRect::top(Val::Px(8.0)),
                column_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            ModifyButton::AckToggle,
            ModifyWarnAckRow,
        ))
        .add_children(&[checkbox, ack_text])
        .id();

    let cancel_label = commands
        .spawn((
            Text::new("キャンセル"),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
        ))
        .id();
    let cancel_btn = commands
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_BTN_CANCEL),
            crate::ui::component::ButtonStyle::Tinted(crate::ui::component::TintColor::Error),
            crate::ui::theme::ElevationIndex::ModalSurface,
            ModifyButton::Cancel,
        ))
        .add_child(cancel_label)
        .id();

    let confirm_label = commands
        .spawn((
            Text::new("Confirm"),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
        ))
        .id();
    let confirm_btn = commands
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(COLOR_BTN_DISABLED),
            crate::ui::component::ButtonStyle::Tinted(crate::ui::component::TintColor::Success),
            crate::ui::theme::ElevationIndex::ModalSurface,
            ModifyButton::Confirm,
        ))
        .add_child(confirm_label)
        .id();

    let btn_row = commands
        .spawn(Node {
            margin: UiRect::top(Val::Px(14.0)),
            column_gap: Val::Px(10.0),
            ..default()
        })
        .add_children(&[cancel_btn, confirm_btn])
        .id();

    commands
        .entity(card)
        .add_children(&[header, warn_row, qty_row, price_row, ack_row, btn_row]);
}

/// ラベル + クリックでフォーカスする入力欄 (背景に focus 色) を 1 行 spawn し、行 Entity を返す。
fn spawn_input_row(
    commands: &mut Commands,
    theme: &Theme,
    label: &str,
    field: ModifyField,
    focus: ModifyButton,
) -> Entity {
    let label_text = commands
        .spawn((
            Node {
                width: Val::Px(64.0),
                ..default()
            },
            Text::new(label.to_string()),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
        ))
        .id();

    let focus_kind = match field {
        ModifyField::Qty => ModifyFocus::Qty,
        ModifyField::Price => ModifyFocus::Price,
    };
    let value_text = commands
        .spawn((
            Text::new(""),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
            field,
        ))
        .id();
    let field_entity = commands
        .spawn((
            Button,
            Node {
                flex_grow: 1.0,
                height: Val::Px(26.0),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(COLOR_FIELD_BG),
            focus,
            ModifyFieldBg(focus_kind),
        ))
        .add_child(value_text)
        .id();

    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .add_children(&[label_text, field_entity])
        .id()
}

// ===========================================================================
// Systems
// ===========================================================================

/// モーダル root の Display を `ModifyForm.open` に同期する。
pub fn modify_modal_visibility_system(
    form: Res<ModifyForm>,
    mut root_q: Query<&mut Node, With<ModifyModalRoot>>,
) {
    let target = if form.open {
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

/// 表示中だけ keyboard を drain して、フォーカス中の数値バッファに反映する。
/// drain により cosmic_edit / picker / menu への二重配送を防ぐ。
/// Tab = フォーカス切替、Enter = Confirm (可能なら)。数字 / `.` のみ受ける。
/// Escape は drain で消費するが破棄はしない (modal_layer_esc_system に委譲, #46 Slice B 5c)。
pub fn modify_modal_input_system(
    mut form: ResMut<ModifyForm>,
    mut kb_events: ResMut<Messages<KeyboardInput>>,
    mut feedback: ResMut<OrderFeedback>,
    sender: Option<Res<TransportCommandSender>>,
) {
    if !form.open {
        return;
    }
    let mut submit = false;
    let mut saw_escape = false;
    for ev in kb_events.drain() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for ch in s.chars() {
                    if ch.is_ascii_digit() || ch == '.' {
                        push_focused(&mut form, ch);
                    }
                }
            }
            Key::Backspace => {
                backspace_focused(&mut form);
            }
            Key::Tab => {
                form.focus = match form.focus {
                    ModifyFocus::Qty => ModifyFocus::Price,
                    ModifyFocus::Price => ModifyFocus::Qty,
                };
            }
            Key::Enter => submit = true,
            // Escape は drain でここで消費し (picker/menu への漏れ防止)、同一フレームの
            // Confirm を抑止する。dismiss 自体は modal_layer_esc_system →
            // modify_modal_reconcile_system に委譲する (#46 Slice B 5c / B2 回帰修正)。
            Key::Escape => saw_escape = true,
            _ => {}
        }
    }
    if submit && !saw_escape {
        do_confirm(&mut form, &mut feedback, sender.as_deref());
    }
}

fn push_focused(form: &mut ModifyForm, ch: char) {
    match form.focus {
        ModifyFocus::Qty => form.new_qty_buf.push(ch),
        ModifyFocus::Price => form.new_price_buf.push(ch),
    }
}

fn backspace_focused(form: &mut ModifyForm) {
    match form.focus {
        ModifyFocus::Qty => {
            form.new_qty_buf.pop();
        }
        ModifyFocus::Price => {
            form.new_price_buf.pop();
        }
    }
}

/// Confirm を実行する。両欄が空 (=変更なし) or kabu 未同意なら弾く。
fn do_confirm(
    form: &mut ModifyForm,
    feedback: &mut OrderFeedback,
    sender: Option<&TransportCommandSender>,
) {
    if !form.can_confirm() {
        let (q, p) = form.parsed();
        if q.is_none() && p.is_none() {
            feedback.message = Some("訂正内容 (数量または価格) を入力してください".to_string());
        } else if form.requires_kabu_ack() && !form.ack_kabu {
            feedback.message =
                Some("kabu の訂正は「理解した上で訂正する」に同意が必要です".to_string());
        }
        return;
    }
    let (new_qty, new_price) = form.parsed();
    feedback.message = None;
    match sender {
        Some(tx) => {
            let _ = tx.tx.send(TransportCommand::ModifyOrder {
                venue: form.venue.clone(),
                client_order_id: form.client_order_id.clone(),
                new_qty,
                new_price,
                // Tachibana 第二暗証番号は SecretRequired で別途収集 (Step 5)。
                second_secret: None,
            });
        }
        None => warn!("ModifyOrder skipped: TransportCommandSender unavailable"),
    }
    form.close();
}

/// ボタン操作 (Confirm / Cancel / ack toggle / フォーカス切替) を処理する。
pub fn modify_modal_button_system(
    interactions: Query<(&Interaction, &ModifyButton), (Changed<Interaction>, With<Button>)>,
    mut form: ResMut<ModifyForm>,
    mut feedback: ResMut<OrderFeedback>,
    sender: Option<Res<TransportCommandSender>>,
) {
    if !form.open {
        return;
    }
    for (interaction, button) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            ModifyButton::Confirm => do_confirm(&mut form, &mut feedback, sender.as_deref()),
            ModifyButton::Cancel => form.close(),
            ModifyButton::AckToggle => form.ack_kabu = !form.ack_kabu,
            ModifyButton::FocusQty => form.focus = ModifyFocus::Qty,
            ModifyButton::FocusPrice => form.focus = ModifyFocus::Price,
        }
    }
}

/// 入力値テキスト・フォーカス背景・警告バナー表示・チェックボックス色・Confirm 色を差分反映する。
pub fn modify_modal_sync_system(
    form: Res<ModifyForm>,
    mut fields: Query<(&ModifyField, &mut Text)>,
    mut field_bgs: Query<(&ModifyFieldBg, &mut BackgroundColor), Without<ModifyAckCheckbox>>,
    mut warn_q: Query<&mut Node, With<ModifyWarnRow>>,
    mut ack_row_q: Query<&mut Node, (With<ModifyWarnAckRow>, Without<ModifyWarnRow>)>,
    mut check_q: Query<&mut BackgroundColor, (With<ModifyAckCheckbox>, Without<ModifyFieldBg>)>,
    confirm_q: Query<
        (
            Entity,
            &ModifyButton,
            Has<crate::ui::component::ButtonDisabled>,
        ),
        (
            With<Button>,
            Without<ModifyFieldBg>,
            Without<ModifyAckCheckbox>,
        ),
    >,
    mut commands: Commands,
) {
    // 入力値テキスト
    for (field, mut text) in &mut fields {
        let new = match field {
            ModifyField::Qty => form.new_qty_buf.clone(),
            ModifyField::Price => form.new_price_buf.clone(),
        };
        if text.0 != new {
            text.0 = new;
        }
    }

    // フォーカス背景
    for (bg_marker, mut bg) in &mut field_bgs {
        let target = if bg_marker.0 == form.focus {
            COLOR_FIELD_BG_ACTIVE
        } else {
            COLOR_FIELD_BG
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    let kabu = form.requires_kabu_ack();

    // 警告バナーとチェック行の表示 (kabu のときだけ)
    let warn_display = if kabu { Display::Flex } else { Display::None };
    if let Ok(mut node) = warn_q.single_mut()
        && node.display != warn_display
    {
        node.display = warn_display;
    }
    if let Ok(mut node) = ack_row_q.single_mut()
        && node.display != warn_display
    {
        node.display = warn_display;
    }

    // チェックボックス色
    if let Ok(mut bg) = check_q.single_mut() {
        let target = if form.ack_kabu {
            COLOR_CHECK_ON
        } else {
            COLOR_CHECK_OFF
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    // Confirm ボタンの gating 視覚化は ButtonDisabled marker に委譲し、
    // button_interaction_system が色を塗る。#46 Slice A。
    let can = form.can_confirm();
    for (entity, button, has_disabled) in &confirm_q {
        if matches!(button, ModifyButton::Confirm) {
            if !can && !has_disabled {
                commands
                    .entity(entity)
                    .insert(crate::ui::component::ButtonDisabled);
            } else if can && has_disabled {
                commands
                    .entity(entity)
                    .remove::<crate::ui::component::ButtonDisabled>();
            }
        }
    }
}

fn modify_dismiss() -> DismissDecision {
    DismissDecision::Dismiss
}

/// `ModalLayer.stack` ⇄ `ModifyForm.open` を双方向同期する (mechanism A, #46 Slice B 5c)。
/// FORWARD: open かつ未登録 → stack に push (dismiss 優先度 z=270)。
/// REVERSE: `modal_layer_esc_system` が entry を pop → `was_on_stack` Local で
/// 検出し form をクリアする (Cancel と同じ cleanup, visibility が hide する)。
pub fn modify_modal_reconcile_system(
    mut form: ResMut<ModifyForm>,
    root_q: Query<Entity, With<ModifyModalRoot>>,
    mut layer: ResMut<ModalLayer>,
    mut was_on_stack: Local<bool>,
) {
    let Ok(root) = root_q.single() else {
        return;
    };
    let is_open = form.open;
    let prompt_changed = form.is_changed();
    reconcile_modal_stack(
        &mut layer,
        root,
        270,
        &mut was_on_stack,
        is_open,
        prompt_changed,
        modify_dismiss,
        || form.close(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<ModifyForm>();
        app.init_resource::<OrderFeedback>();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().spawn(RxHolder { _rx: rx });
        app
    }

    #[derive(Component)]
    struct RxHolder {
        _rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    }

    fn open_form(venue: &str) -> ModifyForm {
        ModifyForm {
            open: true,
            client_order_id: "c1".to_string(),
            venue: venue.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn parse_buf_handles_blank_and_invalid() {
        assert_eq!(parse_buf(""), None);
        assert_eq!(parse_buf("   "), None);
        assert_eq!(parse_buf("abc"), None);
        assert_eq!(parse_buf("0"), None, "non-positive rejected");
        assert_eq!(parse_buf("-5"), None);
        assert_eq!(parse_buf("100"), Some(100.0));
        assert_eq!(parse_buf(" 2500.5 "), Some(2500.5));
    }

    #[test]
    fn can_confirm_requires_a_change() {
        let f = open_form("MOCK");
        assert!(!f.can_confirm(), "no change => cannot confirm");
        let mut f2 = open_form("MOCK");
        f2.new_qty_buf = "200".to_string();
        assert!(f2.can_confirm(), "a qty change is enough");
    }

    #[test]
    fn kabu_gates_confirm_until_ack() {
        let mut f = open_form("kabu");
        f.new_price_buf = "2600".to_string();
        assert!(f.requires_kabu_ack(), "kabu needs ack");
        assert!(!f.can_confirm(), "kabu without ack must be blocked");
        f.ack_kabu = true;
        assert!(f.can_confirm(), "kabu with ack is allowed");
    }

    #[test]
    fn kabu_gate_works_with_backend_uppercase_venue_id() {
        // The venue string actually reaching ModifyForm is VenueStatusRes.venue_id,
        // which the backend reports UPPERCASE ("KABU"). Regression: the gate used to
        // be dead because for_venue only matched lowercase.
        let mut f = open_form("KABU");
        f.new_qty_buf = "200".to_string();
        assert!(f.requires_kabu_ack(), "uppercase KABU must require ack");
        assert!(!f.can_confirm(), "KABU without ack must be blocked");
        f.ack_kabu = true;
        assert!(f.can_confirm());
    }

    #[test]
    fn tachibana_does_not_require_ack() {
        let mut f = open_form("tachibana");
        f.new_qty_buf = "300".to_string();
        assert!(!f.requires_kabu_ack(), "tachibana has CLMKabuCorrectOrder");
        assert!(f.can_confirm());
    }

    #[test]
    fn confirm_fires_modify_order_and_closes() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("MOCK");
            f.new_qty_buf = "200".to_string();
            f.new_price_buf = "2600".to_string();
        }
        app.add_systems(Update, modify_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();

        let cmd = rx.try_recv().expect("Confirm must fire ModifyOrder");
        match cmd {
            TransportCommand::ModifyOrder {
                venue,
                client_order_id,
                new_qty,
                new_price,
                second_secret,
            } => {
                assert_eq!(venue, "MOCK");
                assert_eq!(client_order_id, "c1");
                assert_eq!(new_qty, Some(200.0));
                assert_eq!(new_price, Some(2600.0));
                assert!(second_secret.is_none(), "Step 4 always sends None");
            }
            other => panic!("expected ModifyOrder, got {other:?}"),
        }
        assert!(
            !app.world().resource::<ModifyForm>().open,
            "Confirm must close the modal"
        );
    }

    #[test]
    fn confirm_blocked_when_no_change() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("MOCK"); // both bufs empty
        }
        app.add_systems(Update, modify_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "empty modify must not fire a command"
        );
        assert!(
            app.world().resource::<ModifyForm>().open,
            "modal stays open so the user can enter a value"
        );
        assert!(
            app.world().resource::<OrderFeedback>().message.is_some(),
            "user is told what to enter"
        );
    }

    #[test]
    fn confirm_blocked_for_kabu_without_ack() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("kabu");
            f.new_qty_buf = "200".to_string();
            // ack_kabu stays false
        }
        app.add_systems(Update, modify_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Confirm));
        app.update();
        assert!(rx.try_recv().is_err(), "kabu unack must not fire");
        assert!(app.world().resource::<ModifyForm>().open);
    }

    #[test]
    fn cancel_closes_without_firing() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("MOCK");
            f.new_qty_buf = "200".to_string();
        }
        app.add_systems(Update, modify_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::Cancel));
        app.update();
        assert!(rx.try_recv().is_err(), "Cancel must not fire a command");
        assert!(!app.world().resource::<ModifyForm>().open);
    }

    #[test]
    fn ack_toggle_flips_flag() {
        let mut app = make_app();
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("kabu");
        }
        app.add_systems(Update, modify_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ModifyButton::AckToggle));
        app.update();
        assert!(app.world().resource::<ModifyForm>().ack_kabu);
    }

    use bevy::ecs::system::RunSystemOnce;

    /// Slice 4a RED: modify モーダルも modal skeleton の上に建てる。card に
    /// ElevationIndex::ModalSurface が付くことを要求する。現状 Confirm/Cancel
    /// ボタン 2 個だけが ModalSurface を持つので 2 < 3 で fail → 4b で card が
    /// 3 個目を足して GREEN になる。
    #[test]
    fn modify_modal_card_uses_modal_surface_elevation() {
        use crate::ui::theme::ElevationIndex;
        let mut world = World::new();
        world.insert_resource(crate::ui::theme::Theme::default());
        world.run_system_once(spawn_modify_modal).unwrap();

        let count = world
            .query::<&ElevationIndex>()
            .iter(&world)
            .filter(|e| **e == ElevationIndex::ModalSurface)
            .count();
        assert!(
            count >= 3,
            "card must also carry ElevationIndex::ModalSurface (built via spawn_modal); \
             only the 2 buttons carry it today, got {count}"
        );
    }

    /// #46 Slice B2 回帰 RED: 同一フレームに Enter と Escape が両方届いたとき、
    /// Escape が Confirm に勝つ (cancel-wins) こと。5c 前は Escape 分岐で破棄していたが、
    /// 5c で Escape 分岐を撤去した結果 submit が走り、誤って ModifyOrder が飛ぶ回帰が
    /// 入った。RED→fix で GREEN。
    #[test]
    fn escape_suppresses_same_frame_enter_confirm() {
        use bevy::input::ButtonState;
        use bevy::input::keyboard::KeyCode;

        let mut app = make_app();
        // rx を観測するためローカルに作り直して上書きする (make_app の rx は握り潰し)。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.add_message::<KeyboardInput>();
        {
            let mut f = app.world_mut().resource_mut::<ModifyForm>();
            *f = open_form("MOCK");
            // can_confirm() を true にして do_confirm が早期 return しない条件にする。
            f.new_qty_buf = "200".to_string();
        }
        app.add_systems(Update, modify_modal_input_system);

        // Enter と Escape を同一フレームで投入する。
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: Key::Enter,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "same-frame Escape must suppress Confirm (no ModifyOrder); dismiss is the layer's job"
        );
        assert!(
            app.world().resource::<ModifyForm>().open,
            "form stays open; Escape dismiss is reconcile's job, not this system"
        );
    }
}
