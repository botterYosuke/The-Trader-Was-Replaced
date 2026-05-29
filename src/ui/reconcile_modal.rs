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
use crate::ui::component::modal_layer::{
    ActiveModal, DismissDecision, ModalHandle, ModalLayer, ModalSkeleton, spawn_modal,
};
use crate::ui::theme::{DynamicSpacing, LabelSize, Theme};

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

pub fn spawn_reconcile_modal(mut commands: Commands, theme: Res<Theme>) {
    let density = theme.spacing.density;
    let ModalHandle { root, card } = spawn_modal(
        &mut commands,
        &theme,
        ModalSkeleton {
            width: 420.0,
            z_index: 262,
            name: "ReconcileModal",
        },
    );

    commands.entity(root).insert(ReconcileModalRoot);

    let header = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(DynamicSpacing::Base08.px(density))),
                ..default()
            },
            Text::new("backend 再起動: 注文状態の再確認が必要です"),
            theme.typography.label_font(LabelSize::Large),
            TextColor(theme.status.warning),
        ))
        .id();

    let info = commands
        .spawn((
            Text::new(
                "backend が再起動したため、下記の注文の現在状態が不明になりました。\n\
                 Venue メニューから再ログインし、証券会社側で約定/取消状況を確認してください。",
            ),
            theme.typography.label_font(LabelSize::Small),
            TextColor(theme.colors.text_muted),
        ))
        .id();

    let list = commands
        .spawn((
            Node {
                margin: UiRect::vertical(Val::Px(DynamicSpacing::Base08.px(density))),
                ..default()
            },
            Text::new(""),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
            ReconcileListText,
        ))
        .id();

    let dismiss_label = commands
        .spawn((
            Text::new("確認した"),
            theme.typography.label_font(LabelSize::Large),
            TextColor(theme.colors.text),
        ))
        .id();
    let dismiss_btn = commands
        .spawn((
            Button,
            Node {
                width: Val::Px(96.0),
                height: Val::Px(30.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.colors.element_selection_background),
            ReconcileDismissButton,
        ))
        .add_child(dismiss_label)
        .id();
    let btn_row = commands
        .spawn(Node {
            justify_content: JustifyContent::FlexEnd,
            ..default()
        })
        .add_child(dismiss_btn)
        .id();

    commands
        .entity(card)
        .add_children(&[header, info, list, btn_row]);
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

/// [確認した] ボタンで通知を消す (unknown をクリア)。Escape は
/// `modal_layer_esc_system` (汎用 modal-layer Esc) + `reconcile_modal_reconcile_system`
/// 経由で処理する (#46 B3, relogin と同方針)。
pub fn reconcile_modal_button_system(
    interactions: Query<&Interaction, (Changed<Interaction>, With<ReconcileDismissButton>)>,
    mut prompt: ResMut<ReconcilePrompt>,
) {
    if prompt.unknown.is_empty() {
        return;
    }
    if interactions.iter().any(|i| *i == Interaction::Pressed) {
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
    if let Ok(mut t) = list_q.single_mut()
        && t.0 != body
    {
        t.0 = body;
    }
}

/// reconcile notice の on_before_dismiss フック。通知モーダルは
/// work-in-flight を持たないので常に Dismiss。
fn reconcile_dismiss() -> DismissDecision {
    DismissDecision::Dismiss
}

/// ModalLayer.stack ⇄ ReconcilePrompt.unknown を双方向同期する（mechanism A、relogin と同形）。
/// FORWARD: open(!unknown.is_empty()) かつ未登録 → push。CLOSE: 空 & on_stack → 除去。
/// REVERSE: 前フレーム on_stack で今 off かつ open → unknown.clear()（esc pop の逆反映）。
pub fn reconcile_modal_reconcile_system(
    mut prompt: ResMut<ReconcilePrompt>,
    root_q: Query<Entity, With<ReconcileModalRoot>>,
    mut layer: ResMut<ModalLayer>,
    mut was_on_stack: Local<bool>,
) {
    let Ok(root) = root_q.single() else {
        return;
    };
    let on_stack = layer.stack.iter().any(|m| m.root == root);

    if prompt.is_changed() && !prompt.unknown.is_empty() && !on_stack {
        layer.push(ActiveModal {
            root,
            backdrop: root,
            previous_focus: None,
            on_before_dismiss: reconcile_dismiss,
        });
        *was_on_stack = true;
        return;
    }

    if prompt.unknown.is_empty() && on_stack {
        layer.stack.retain(|m| m.root != root);
        *was_on_stack = false;
        return;
    }

    if *was_on_stack && !on_stack && !prompt.unknown.is_empty() {
        prompt.unknown.clear();
    }

    *was_on_stack = on_stack;
}

/// 不明注文を「symbol id (status)」の行リストに整形する純関数（テスト用に分離）。
fn format_unknown_list(prompt: &ReconcilePrompt) -> String {
    prompt
        .unknown
        .iter()
        .map(|o| {
            format!(
                "• {} {} (最後の状態: {})",
                o.symbol, o.client_order_id, o.status
            )
        })
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
        let id = app
            .world_mut()
            .spawn((Text::new(""), ReconcileListText))
            .id();
        app.add_systems(Update, reconcile_modal_sync_system);
        app.update();
        let text = app.world().get::<Text>(id).unwrap();
        assert!(text.0.contains("c1"));
        assert!(text.0.contains("c2"));
        assert!(text.0.contains("7203.T"));
    }
}
