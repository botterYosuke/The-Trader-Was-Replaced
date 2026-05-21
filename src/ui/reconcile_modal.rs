//! Phase 9 §3.8 — backend 再起動後の in-flight 注文 reconcile 通知モーダル。
//!
//! supervisor が crash した backend を自動再起動して再び `Ready` になると、
//! `backend_restart_resync_system` (backend_sync.rs) が `GetOrders` を撃ち、応答を
//! `apply_status_update` が UI の楽観的 `LiveOrders` と diff して `ReconcilePrompt.unknown`
//! を埋める。本モーダルは `unknown` が非空の間だけ開き、「これらの注文の状態が backend
//! 再起動で不明になった」ことをユーザーに伝える。
//!
//! **設計判断 (relogin_modal と同方針)**: モーダルは **通知に徹する**。再起動直後の
//! backend は venue 未ログインなので、ここから自動で注文を取り消す/再送するのは危険
//! (二重発注リスク, ADR §3.8)。ユーザーは Venue メニューで再ログインし、venue 側で実際の
//! 注文状態を確認する。UI Node 流派 (relogin_modal / secret_modal と同系統)。

use bevy::prelude::*;

use crate::trading::ReconcilePrompt;

const COLOR_PANEL_BG: Color = Color::srgba(0.07, 0.07, 0.12, 0.98);
const COLOR_BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const COLOR_HEADER: Color = Color::srgb(1.0, 0.62, 0.20);
const COLOR_INFO: Color = Color::srgb(0.78, 0.81, 0.86);
const COLOR_VALUE: Color = Color::srgb(0.88, 0.91, 0.96);
const COLOR_BTN: Color = Color::srgba(0.16, 0.30, 0.44, 1.0);

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct ReconcileModalRoot;

/// 不明になった注文の一覧を差し込む行。
#[derive(Component)]
pub struct ReconcileListText;

#[derive(Component)]
pub struct ReconcileDismissButton;

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

pub fn spawn_reconcile_modal(mut commands: Commands) {
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
            // relogin modal (260) と同級。secret modal (300) より前面である必要はない。
            GlobalZIndex(262),
            ReconcileModalRoot,
            Name::new("ReconcileModal"),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(420.0),
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
                    Text::new("backend 再起動: 注文状態の再確認が必要です"),
                    TextFont {
                        font_size: 15.0,
                        ..default()
                    },
                    TextColor(COLOR_HEADER),
                ));
                card.spawn((
                    Text::new(
                        "backend が再起動したため、下記の注文の現在状態が不明になりました。\n\
                         Venue メニューから再ログインし、証券会社側で約定/取消状況を確認してください。",
                    ),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(COLOR_INFO),
                ));
                card.spawn((
                    Node {
                        margin: UiRect::vertical(Val::Px(8.0)),
                        ..default()
                    },
                    Text::new(""),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(COLOR_VALUE),
                    ReconcileListText,
                ));
                card.spawn((Node {
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                },))
                    .with_children(|btns| {
                        btns.spawn((
                            Button,
                            Node {
                                width: Val::Px(96.0),
                                height: Val::Px(30.0),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(COLOR_BTN),
                            ReconcileDismissButton,
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new("確認した"),
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

/// モーダル root の Display を `ReconcilePrompt.unknown` の有無に同期する。
pub fn reconcile_modal_visibility_system(
    prompt: Res<ReconcilePrompt>,
    mut root_q: Query<&mut Node, With<ReconcileModalRoot>>,
) {
    if !prompt.is_changed() {
        return;
    }
    let target = if prompt.unknown.is_empty() {
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

/// [確認した] ボタン / Esc で通知を消す (unknown をクリア)。
pub fn reconcile_modal_button_system(
    interactions: Query<&Interaction, (Changed<Interaction>, With<ReconcileDismissButton>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut prompt: ResMut<ReconcilePrompt>,
) {
    if prompt.unknown.is_empty() {
        return;
    }
    let pressed = interactions.iter().any(|i| *i == Interaction::Pressed);
    if pressed || keys.just_pressed(KeyCode::Escape) {
        prompt.unknown.clear();
    }
}

/// 不明注文の一覧を差分反映する (規約 2: 変化時のみ書く)。
pub fn reconcile_modal_sync_system(
    prompt: Res<ReconcilePrompt>,
    mut list_q: Query<&mut Text, With<ReconcileListText>>,
) {
    if !prompt.is_changed() {
        return;
    }
    let body = format_unknown_list(&prompt);
    if let Ok(mut t) = list_q.get_single_mut()
        && t.0 != body
    {
        t.0 = body;
    }
}

/// 不明注文を「symbol id (status)」の行リストに整形する純関数（テスト用に分離）。
fn format_unknown_list(prompt: &ReconcilePrompt) -> String {
    prompt
        .unknown
        .iter()
        .map(|o| format!("• {} {} (最後の状態: {})", o.symbol, o.client_order_id, o.status))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::ReconcileUnknownOrder;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<ReconcilePrompt>();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app
    }

    fn unknown(id: &str) -> ReconcileUnknownOrder {
        ReconcileUnknownOrder {
            client_order_id: id.to_string(),
            symbol: "7203.T".to_string(),
            status: "ACCEPTED".to_string(),
        }
    }

    #[test]
    fn dismiss_button_clears_prompt() {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![unknown("c1")];
        app.add_systems(Update, reconcile_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();
        assert!(
            app.world().resource::<ReconcilePrompt>().unknown.is_empty(),
            "確認した must clear the reconcile prompt"
        );
    }

    #[test]
    fn escape_clears_prompt() {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReconcilePrompt>().unknown = vec![unknown("c1")];
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.add_systems(Update, reconcile_modal_button_system);
        app.update();
        assert!(app.world().resource::<ReconcilePrompt>().unknown.is_empty());
    }

    #[test]
    fn button_system_noop_when_closed() {
        let mut app = make_app();
        app.add_systems(Update, reconcile_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReconcileDismissButton));
        app.update();
        assert!(app.world().resource::<ReconcilePrompt>().unknown.is_empty());
    }

    #[test]
    fn sync_writes_orders_into_list_line() {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReconcilePrompt>().unknown =
            vec![unknown("c1"), unknown("c2")];
        let id = app.world_mut().spawn((Text::new(""), ReconcileListText)).id();
        app.add_systems(Update, reconcile_modal_sync_system);
        app.update();
        let text = app.world().get::<Text>(id).unwrap();
        assert!(text.0.contains("c1"));
        assert!(text.0.contains("c2"));
        assert!(text.0.contains("7203.T"));
    }
}
