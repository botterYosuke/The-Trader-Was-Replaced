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

const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.98);
const COLOR_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
const COLOR_HEADER: Color = Color::srgb(0.0, 0.81, 1.0);
const COLOR_LABEL: Color = Color::srgb(0.65, 0.70, 0.78);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_WARN_BG: Color = Color::srgba(0.35, 0.22, 0.05, 1.0);
const COLOR_WARN_TEXT: Color = Color::srgb(1.0, 0.78, 0.35);
const COLOR_FIELD_BG: Color = Color::srgba(0.04, 0.04, 0.08, 1.0);
const COLOR_FIELD_BG_ACTIVE: Color = Color::srgba(0.10, 0.14, 0.22, 1.0);
const COLOR_BTN_SUBMIT: Color = Color::srgba(0.10, 0.45, 0.30, 1.0);
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

pub fn spawn_modify_modal(mut commands: Commands) {
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
            // 確認モーダル (200) より前面、secret modal (300) より背面。
            GlobalZIndex(250),
            ModifyModalRoot,
            Name::new("ModifyModal"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(340.0),
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
                    Text::new("注文の訂正"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(COLOR_HEADER),
                    ModifyTitleText,
                ));

                // kabu 警告バナー (Display で出し入れ)
                card.spawn((
                    Node {
                        display: Display::None,
                        width: Val::Percent(100.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        margin: UiRect::bottom(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(COLOR_WARN_BG),
                    ModifyWarnRow,
                ))
                .with_children(|w| {
                    w.spawn((
                        Text::new(KABU_WARNING),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(COLOR_WARN_TEXT),
                    ));
                });

                // 数量入力欄
                spawn_input_row(card, "新数量:", ModifyField::Qty, ModifyButton::FocusQty);
                // 価格入力欄
                spawn_input_row(
                    card,
                    "新価格:",
                    ModifyField::Price,
                    ModifyButton::FocusPrice,
                );

                // kabu 同意チェックボックス行 (kabu のときだけ可視: visibility system が制御)
                card.spawn((
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
                .with_children(|row| {
                    row.spawn((
                        Node {
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            ..default()
                        },
                        BackgroundColor(COLOR_CHECK_OFF),
                        ModifyAckCheckbox,
                    ));
                    row.spawn((
                        Text::new("理解した上で訂正する"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(COLOR_LABEL),
                        ModifyAckText,
                    ));
                });

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
                            ModifyButton::Cancel,
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
                            BackgroundColor(COLOR_BTN_DISABLED),
                            ModifyButton::Confirm,
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

/// ラベル + クリックでフォーカスする入力欄 (背景に focus 色) を 1 行 spawn する。
fn spawn_input_row(
    parent: &mut ChildBuilder,
    label: &str,
    field: ModifyField,
    focus: ModifyButton,
) {
    parent
        .spawn((Node {
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        },))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: Val::Px(64.0),
                    ..default()
                },
                Text::new(label.to_string()),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(COLOR_LABEL),
            ));
            let focus_kind = match field {
                ModifyField::Qty => ModifyFocus::Qty,
                ModifyField::Price => ModifyFocus::Price,
            };
            row.spawn((
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
            .with_children(|f| {
                f.spawn((
                    Text::new(""),
                    TextFont {
                        font_size: 13.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    field,
                ));
            });
        });
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
/// Tab = フォーカス切替、Enter = Confirm (可能なら)、Esc = 破棄。数字 / `.` のみ受ける。
pub fn modify_modal_input_system(
    mut form: ResMut<ModifyForm>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
    mut feedback: ResMut<OrderFeedback>,
    sender: Option<Res<TransportCommandSender>>,
) {
    if !form.open {
        return;
    }
    let mut submit = false;
    let mut cancel = false;
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
            Key::Escape => cancel = true,
            _ => {}
        }
    }
    if cancel {
        form.close();
    } else if submit {
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
    mut confirm_q: Query<
        (&ModifyButton, &mut BackgroundColor),
        (
            With<Button>,
            Without<ModifyFieldBg>,
            Without<ModifyAckCheckbox>,
        ),
    >,
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
    if let Ok(mut node) = warn_q.get_single_mut()
        && node.display != warn_display
    {
        node.display = warn_display;
    }
    if let Ok(mut node) = ack_row_q.get_single_mut()
        && node.display != warn_display
    {
        node.display = warn_display;
    }

    // チェックボックス色
    if let Ok(mut bg) = check_q.get_single_mut() {
        let target = if form.ack_kabu {
            COLOR_CHECK_ON
        } else {
            COLOR_CHECK_OFF
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }

    // Confirm ボタン色 (gating の視覚化)
    let can = form.can_confirm();
    for (button, mut bg) in &mut confirm_q {
        if matches!(button, ModifyButton::Confirm) {
            let target = if can {
                COLOR_BTN_SUBMIT
            } else {
                COLOR_BTN_DISABLED
            };
            if bg.0 != target {
                bg.0 = target;
            }
        }
    }
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
}
