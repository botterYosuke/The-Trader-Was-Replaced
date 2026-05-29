//! OrderPanel の 2 段階確認モーダル (Phase 9 §3.9)。order_panel/mod.rs から分割。
//! 中央オーバーレイの確認 UI・確認系 systems・PlaceOrder 発射。

use bevy::prelude::*;

use crate::trading::{
    LastPrices, OrderFeedback, SecretPrompt, TransportCommand, TransportCommandSender,
};
use crate::ui::component::modal_layer::{ModalHandle, ModalSkeleton, spawn_modal};
use crate::ui::theme::{LabelSize, Theme};

use super::form::{OrderDraft, estimated_notional};

/// 2 段階確認の状態。`pending` が `Some` の間だけ確認モーダルを出す。
#[derive(Resource, Default, Debug, Clone)]
pub struct OrderConfirm {
    pub pending: Option<OrderDraft>,
    /// 発注ボタン押下時の検証エラー (パネルに赤字表示)。成功時は `None`。
    pub last_error: Option<String>,
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

/// 2 段階確認モーダル (中央オーバーレイ) を spawn する (Startup)。初期 Display は None。
/// modal skeleton (spawn_modal) の上に建てるので card は ElevationIndex::ModalSurface を
/// 持ち、backdrop / card 背景は theme トークン由来になる (#46 Slice B Step 2)。
pub fn spawn_confirm_modal(mut commands: Commands, theme: Res<Theme>) {
    let ModalHandle { root, card } = spawn_modal(
        &mut commands,
        &theme,
        ModalSkeleton {
            width: 320.0,
            z_index: 200,
            name: "OrderConfirmModal",
        },
    );

    commands.entity(root).insert(ConfirmModalRoot);

    let header = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(10.0)),
                ..default()
            },
            Text::new("発注内容の確認"),
            theme.typography.label_font(LabelSize::Large),
            TextColor(theme.colors.text_accent),
        ))
        .id();

    // 内容サマリ (sync system が書き換える)
    let summary = commands
        .spawn((
            Text::new(""),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
            ConfirmSummary,
        ))
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
            BackgroundColor(theme.colors.element_selection_background),
            ConfirmButton::Cancel,
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
            BackgroundColor(theme.colors.element_selection_background),
            ConfirmButton::Confirm,
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
        .add_children(&[header, summary, btn_row]);
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

/// 確認モーダルのサマリテキストを `pending` ドラフトから差分反映する。
pub fn confirm_modal_sync_system(
    confirm: Res<OrderConfirm>,
    last_prices: Res<LastPrices>,
    mut summary_q: Query<&mut Text, With<ConfirmSummary>>,
) {
    let Some(draft) = confirm.pending.as_ref() else {
        return;
    };
    let Ok(mut text) = summary_q.single_mut() else {
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
    use super::super::form::{
        OrderButton, OrderButtonPressed, OrderForm, Side, OrderType, TimeInForce,
        order_submit_button_system,
    };
    use crate::trading::{SelectedSymbol, VenueStatusRes};

    fn make_app() -> App {
        let mut app = App::new();
        app.add_message::<OrderButtonPressed>();
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
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
        app.update();
        assert!(
            app.world().resource::<OrderConfirm>().pending.is_some(),
            "valid submit must open the confirm modal"
        );
    }

    #[test]
    fn submit_sets_error_when_symbol_missing() {
        let mut app = make_app();
        app.world_mut().resource_mut::<SelectedSymbol>().id = None;
        app.add_systems(Update, order_submit_button_system);
        app.world_mut()
            .write_message(OrderButtonPressed(OrderButton::Submit));
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

    use bevy::ecs::system::RunSystemOnce;

    /// Slice 2a RED: 確認モーダルは modal skeleton の上に建てる。card に
    /// ElevationIndex::ModalSurface が付くことを要求する（現状 spawn_confirm_modal は
    /// ElevationIndex を一切付けないので runtime で fail する → 2b で GREEN）。
    #[test]
    fn confirm_modal_card_uses_modal_surface_elevation() {
        use crate::ui::theme::ElevationIndex;
        let mut world = World::new();
        world.insert_resource(crate::ui::theme::Theme::default());
        world.run_system_once(spawn_confirm_modal).unwrap();

        let found = world
            .query::<&ElevationIndex>()
            .iter(&world)
            .any(|e| *e == ElevationIndex::ModalSurface);
        assert!(
            found,
            "confirm modal card must carry ElevationIndex::ModalSurface (built via spawn_modal)"
        );
    }
}
