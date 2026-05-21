//! Phase 9 §3.12 (Step 4) — OrdersPanel 右クリックコンテキストメニュー ([取消] / [訂正])。
//!
//! OrdersPanel は world-space Sprite/Text2d パネル (`orders.rs`)。各データ行に透明 Sprite の
//! ヒット領域 (`OrdersRowHit`) を貼り、`Pointer<Down>` observer が **Secondary (右) ボタンのみ**
//! 反応して、対象行の `client_order_id` / venue / カーソル位置を本 `OrderContextMenu` resource に
//! セットする (`orders.rs` 側)。本モジュールは Bevy UI Node オーバーレイのメニューを
//! カーソル付近に開き、[取消] / [訂正] のクリックを処理する。
//!
//! - [取消] → `TransportCommand::CancelOrder { venue, order_id: client_order_id, second_secret: None }`
//!   (CancelOrder は Step 3 で配線済み)。メニューを閉じる。
//! - [訂正] → Modify モーダル (`modify_modal.rs`) を `client_order_id` / venue 付きで開く。閉じる。
//! - パネル外クリック (backdrop) / Esc で閉じる。
//!
//! 表示は UI Node 流派 (Display::Flex/None、`GlobalZIndex` で前面化)。

use bevy::prelude::*;

use crate::trading::{TransportCommand, TransportCommandSender};
use crate::ui::modify_modal::{ModifyFocus, ModifyForm};

const COLOR_MENU_BG: Color = Color::srgba(0.10, 0.11, 0.16, 0.99);
const COLOR_ITEM_TEXT: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_ITEM_HOVER: Color = Color::srgba(0.10, 0.40, 0.60, 1.0);
const COLOR_ITEM_IDLE: Color = Color::srgba(0.0, 0.0, 0.0, 0.0);

const MENU_WIDTH: f32 = 120.0;

// ===========================================================================
// Resource
// ===========================================================================

/// 右クリックで開くコンテキストメニューの状態。`orders.rs` の row hit observer が
/// セットし、本モジュールの systems が読む。
#[derive(Resource, Default, Debug, Clone)]
pub struct OrderContextMenu {
    pub open: bool,
    pub client_order_id: Option<String>,
    pub venue: String,
    /// スクリーン座標 (UI Node の絶対 top/left に使う。0.15 のスクリーン座標は左上原点で
    /// UI と同じ向き)。
    pub screen_pos: Vec2,
}

impl OrderContextMenu {
    fn close(&mut self) {
        self.open = false;
        self.client_order_id = None;
        self.venue.clear();
    }
}

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct ContextMenuRoot;

/// 背景全面の透明クリックキャッチャ (パネル外クリックで閉じる)。
#[derive(Component)]
pub struct ContextMenuBackdrop;

/// メニューパネル本体 (位置をカーソルに追従させる)。
#[derive(Component)]
pub struct ContextMenuPanel;

#[derive(Component, Clone, Copy)]
pub enum ContextMenuItem {
    Cancel,
    Modify,
}

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

pub fn spawn_order_context_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // backdrop は完全透明だが UI Interaction を拾うために存在する。
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
            GlobalZIndex(220),
            ContextMenuRoot,
            Name::new("OrderContextMenu"),
        ))
        .with_children(|root| {
            // 全面 backdrop (パネル外クリックで閉じる)。Button にして Interaction を拾う。
            root.spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.0)),
                ContextMenuBackdrop,
            ));
            // メニュー本体 (位置は sync system がカーソルに合わせる)。
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Px(MENU_WIDTH),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(COLOR_MENU_BG),
                ContextMenuPanel,
            ))
            .with_children(|panel| {
                spawn_item(panel, ContextMenuItem::Cancel, "取消");
                spawn_item(panel, ContextMenuItem::Modify, "訂正");
            });
        });
}

fn spawn_item(parent: &mut ChildBuilder, item: ContextMenuItem, label: &str) {
    parent
        .spawn((
            Button,
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(26.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(8.0)),
                ..default()
            },
            BackgroundColor(COLOR_ITEM_IDLE),
            item,
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(COLOR_ITEM_TEXT),
            ));
        });
}

// ===========================================================================
// Systems
// ===========================================================================

/// メニュー root の Display を `OrderContextMenu.open` に同期し、パネル位置をカーソルに合わせる。
pub fn context_menu_visibility_system(
    menu: Res<OrderContextMenu>,
    mut root_q: Query<&mut Node, (With<ContextMenuRoot>, Without<ContextMenuPanel>)>,
    mut panel_q: Query<&mut Node, (With<ContextMenuPanel>, Without<ContextMenuRoot>)>,
) {
    let target = if menu.open {
        Display::Flex
    } else {
        Display::None
    };
    if let Ok(mut node) = root_q.get_single_mut()
        && node.display != target
    {
        node.display = target;
    }
    if menu.open
        && let Ok(mut panel) = panel_q.get_single_mut()
    {
        let left = Val::Px(menu.screen_pos.x);
        let top = Val::Px(menu.screen_pos.y);
        if panel.left != left {
            panel.left = left;
        }
        if panel.top != top {
            panel.top = top;
        }
    }
}

/// Esc でメニューを閉じる。
pub fn context_menu_keyboard_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut menu: ResMut<OrderContextMenu>,
) {
    if menu.open && keys.just_pressed(KeyCode::Escape) {
        menu.close();
    }
}

/// 項目クリック / backdrop クリックを処理する。
/// - [取消] → `CancelOrder` を発射してメニューを閉じる。
/// - [訂正] → `ModifyForm` を開いてメニューを閉じる。
/// - backdrop → 閉じるだけ。
pub fn context_menu_item_system(
    item_q: Query<(&Interaction, &ContextMenuItem), (Changed<Interaction>, With<Button>)>,
    backdrop_q: Query<&Interaction, (Changed<Interaction>, With<ContextMenuBackdrop>)>,
    mut menu: ResMut<OrderContextMenu>,
    mut modify_form: ResMut<ModifyForm>,
    sender: Option<Res<TransportCommandSender>>,
) {
    if !menu.open {
        return;
    }
    for (interaction, item) in &item_q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(client_order_id) = menu.client_order_id.clone() else {
            menu.close();
            return;
        };
        match item {
            ContextMenuItem::Cancel => match sender.as_ref() {
                Some(tx) => {
                    let _ = tx.tx.send(TransportCommand::CancelOrder {
                        venue: menu.venue.clone(),
                        order_id: client_order_id,
                        second_secret: None,
                    });
                }
                None => warn!("CancelOrder skipped: TransportCommandSender unavailable"),
            },
            ContextMenuItem::Modify => {
                modify_form.open = true;
                modify_form.client_order_id = client_order_id;
                modify_form.venue = menu.venue.clone();
                modify_form.new_qty_buf.clear();
                modify_form.new_price_buf.clear();
                modify_form.ack_kabu = false;
                modify_form.focus = ModifyFocus::Qty;
            }
        }
        menu.close();
        // An item handled this frame's click; the backdrop check below is only
        // for clicks that landed outside the menu panel.
        return;
    }
    for interaction in &backdrop_q {
        if *interaction == Interaction::Pressed {
            menu.close();
            return;
        }
    }
}

/// ホバー中の項目を色付けする (差分書き込み)。
pub fn context_menu_hover_system(
    mut item_q: Query<(&Interaction, &mut BackgroundColor), (With<ContextMenuItem>, With<Button>)>,
) {
    for (interaction, mut bg) in &mut item_q {
        let target = match interaction {
            Interaction::Hovered | Interaction::Pressed => COLOR_ITEM_HOVER,
            Interaction::None => COLOR_ITEM_IDLE,
        };
        if bg.0 != target {
            bg.0 = target;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<OrderContextMenu>();
        app.init_resource::<ModifyForm>();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.world_mut().spawn(RxHolder { _rx: rx });
        app
    }

    #[derive(Component)]
    struct RxHolder {
        _rx: tokio::sync::mpsc::UnboundedReceiver<TransportCommand>,
    }

    fn open_menu(app: &mut App) {
        let mut menu = app.world_mut().resource_mut::<OrderContextMenu>();
        menu.open = true;
        menu.client_order_id = Some("c1".to_string());
        menu.venue = "MOCK".to_string();
    }

    #[test]
    fn cancel_item_fires_cancel_order_and_closes() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        open_menu(&mut app);
        app.add_systems(Update, context_menu_item_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();

        let cmd = rx.try_recv().expect("Cancel item must fire CancelOrder");
        match cmd {
            TransportCommand::CancelOrder {
                venue,
                order_id,
                second_secret,
            } => {
                assert_eq!(venue, "MOCK");
                assert_eq!(order_id, "c1");
                assert!(second_secret.is_none());
            }
            other => panic!("expected CancelOrder, got {other:?}"),
        }
        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "menu must close after Cancel"
        );
    }

    #[test]
    fn modify_item_opens_modify_form_and_closes_menu() {
        let mut app = make_app();
        open_menu(&mut app);
        app.add_systems(Update, context_menu_item_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Modify));
        app.update();

        let form = app.world().resource::<ModifyForm>();
        assert!(form.open, "Modify must open the ModifyForm");
        assert_eq!(form.client_order_id, "c1");
        assert_eq!(form.venue, "MOCK");
        assert!(
            !app.world().resource::<OrderContextMenu>().open,
            "menu must close after Modify"
        );
    }

    #[test]
    fn esc_closes_menu() {
        let mut app = make_app();
        open_menu(&mut app);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.add_systems(Update, context_menu_keyboard_system);
        app.update();
        assert!(!app.world().resource::<OrderContextMenu>().open);
    }

    #[test]
    fn item_click_is_noop_when_menu_closed() {
        let mut app = make_app();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(TransportCommandSender { tx });
        app.add_systems(Update, context_menu_item_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ContextMenuItem::Cancel));
        app.update();
        assert!(rx.try_recv().is_err(), "closed menu must not fire commands");
    }
}
