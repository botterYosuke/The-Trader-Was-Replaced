//! Phase 9 §3.5 / Step 7 — 再ログイン通知モーダル (venue 本体ログアウト検知)。
//!
//! `SubscribeBackendEvents` の `VenueLogoutDetected` を `backend_event_drain_system`
//! (backend_sync.rs) が `ReloginPrompt.active = Some(venue)` にセット → 本モーダルが開く。
//! kabu は VenueHealthWatchdog (GET /apisoftlimit poll), Tachibana は EVENT WS の SS=閉局
//! フレームで検知する (どちらも backend が同じ `VenueLogoutDetected` に正規化して push)。
//!
//! **設計判断 (drift note, §3.5)**: 計画書は「再ログイン modal → ログイン完了で購読再開」と
//! 書くが、本モーダルは **通知に徹し自身は `VenueLogin` を発射しない**。検知時点で backend の
//! `venue_sm` はまだ `CONNECTED`（検知は push で状態遷移ではない）なので、ここから直接
//! `VenueLogin` を撃つと busy slot に衝突する。さらに環境 (demo/verify/prod) 選択は Venue
//! メニューが所有しており、モーダルから環境を推測して撃つと**誤った環境への再接続**になりうる。
//! よって実際の再ログインは既存の Venue メニュー (Disconnect→Connect) を通す——購読再開も
//! その既存ログインフローが担う。モーダルは「落ちた事実と次の操作」をユーザーに伝える役割。
//! UI Node 流派 (secret_modal / order_panel と同系統)。表示は `Display::Flex/None`。

use bevy::prelude::*;

use crate::trading::ReloginPrompt;
use crate::ui::component::modal_layer::{
    ActiveModal, DismissDecision, ModalHandle, ModalLayer, ModalSkeleton, spawn_modal,
};
use crate::ui::theme::{DynamicSpacing, LabelSize, Theme};

// ===========================================================================
// Components
// ===========================================================================

#[derive(Component)]
pub struct ReloginModalRoot;

/// venue 名を差し込む情報行。
#[derive(Component)]
pub struct ReloginInfoText;

#[derive(Component)]
pub struct ReloginDismissButton;

// ===========================================================================
// Spawn (Startup)
// ===========================================================================

pub fn spawn_relogin_modal(mut commands: Commands, theme: Res<Theme>) {
    let density = theme.spacing.density;
    let ModalHandle { root, card } = spawn_modal(
        &mut commands,
        &theme,
        ModalSkeleton {
            width: 360.0,
            z_index: 260,
            name: "ReloginModal",
        },
    );

    commands.entity(root).insert(ReloginModalRoot);

    // NOTE: 記号グリフ (⚠ 等) は既定フォントに無く □ 落ちする
    // (footer は ▶/■ 用に NotoSansSymbols2 を別ロードしている)。
    // 警告色 (theme.status.warning=オレンジ) で代替し、本文は Basic-Latin+JP のみ。
    let header = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(DynamicSpacing::Base08.px(density))),
                ..default()
            },
            Text::new("venue からログアウトされました"),
            theme.typography.label_font(LabelSize::Large),
            TextColor(theme.status.warning),
        ))
        .id();

    let info = commands
        .spawn((
            Node {
                margin: UiRect::bottom(Val::Px(DynamicSpacing::Base06.px(density))),
                ..default()
            },
            Text::new(""),
            theme.typography.label_font(LabelSize::Default),
            TextColor(theme.colors.text),
            ReloginInfoText,
        ))
        .id();

    let body = commands
        .spawn((
            Text::new(
                "メニューの Venue → Disconnect の後、Connect から再ログインしてください。\n\
                 Venue メニューから再ログインすると購読は自動的に再開されます。",
            ),
            theme.typography.label_font(LabelSize::Small),
            TextColor(theme.colors.text_muted),
        ))
        .id();

    let dismiss_label = commands
        .spawn((
            Text::new("閉じる"),
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
            ReloginDismissButton,
        ))
        .add_child(dismiss_label)
        .id();
    let btn_row = commands
        .spawn(Node {
            margin: UiRect::top(Val::Px(DynamicSpacing::Base12.px(density))),
            justify_content: JustifyContent::FlexEnd,
            ..default()
        })
        .add_child(dismiss_btn)
        .id();

    commands
        .entity(card)
        .add_children(&[header, info, body, btn_row]);
}

// ===========================================================================
// Systems
// ===========================================================================

/// モーダル root の Display を `ReloginPrompt.active` に同期する。
pub fn relogin_modal_visibility_system(
    prompt: Res<ReloginPrompt>,
    mut root_q: Query<&mut Node, With<ReloginModalRoot>>,
) {
    // Display は ReloginPrompt の変化時のみ追従すれば足りる (モーダルはほぼ常時閉じている)。
    if !prompt.is_changed() {
        return;
    }
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

/// [閉じる] ボタンで通知を消す (prompt をクリア)。
///
/// Escape による dismiss は B2-4 step 2 で `modal_layer_esc_system` に移管した。
/// ここはボタンクリック専用。Escape は reconcile の REVERSE sync 経由で prompt を消す。
pub fn relogin_modal_button_system(
    interactions: Query<&Interaction, (Changed<Interaction>, With<ReloginDismissButton>)>,
    mut prompt: ResMut<ReloginPrompt>,
) {
    if prompt.active.is_none() {
        return;
    }
    if interactions.iter().any(|i| *i == Interaction::Pressed) {
        prompt.active = None;
    }
}

/// venue 名を情報行に差分反映する (規約 2: 変化時のみ書く)。
pub fn relogin_modal_sync_system(
    prompt: Res<ReloginPrompt>,
    mut info_q: Query<&mut Text, With<ReloginInfoText>>,
) {
    // venue 名は ReloginPrompt の変化時のみ組み直す (毎フレームの format! 確保を避ける)。
    if !prompt.is_changed() {
        return;
    }
    let info = match prompt.active.as_ref() {
        Some(venue) => format!("venue: {venue}"),
        None => String::new(),
    };
    if let Ok(mut t) = info_q.single_mut()
        && t.0 != info
    {
        t.0 = info;
    }
}

/// relogin notice の `on_before_dismiss` フック。通知モーダルは work-in-flight を
/// 持たないので常に [`DismissDecision::Dismiss`] を返す（prod 用 free fn。
/// modal_layer の `dismiss` テストヘルパは `#[cfg(test)]` なので prod から使えない）。
fn relogin_dismiss() -> DismissDecision {
    DismissDecision::Dismiss
}

/// `ModalLayer.stack` ⇄ `ReloginPrompt.active` を双方向同期する (mechanism A, B2-4 step 2)。
///
/// - FORWARD (open): `prompt.is_changed()` で active=Some かつ未登録 → stack に push。
/// - REVERSE (esc dismiss): `modal_layer_esc_system` が自分の entry を pop すると
///   prompt は変化しない。`was_on_stack` Local で「前フレーム stack に居た → 今フレーム
///   居ない (active はまだ Some)」を検出し prompt をクリアする (visibility が hide する)。
pub fn relogin_modal_reconcile_system(
    mut prompt: ResMut<ReloginPrompt>,
    root_q: Query<Entity, With<ReloginModalRoot>>,
    mut layer: ResMut<ModalLayer>,
    mut was_on_stack: Local<bool>,
) {
    let Ok(root) = root_q.single() else {
        return;
    };
    let on_stack = layer.stack.iter().any(|m| m.root == root);

    if prompt.is_changed() && prompt.active.is_some() && !on_stack {
        layer.push(ActiveModal {
            root,
            backdrop: root,
            previous_focus: None,
            on_before_dismiss: relogin_dismiss,
        });
        *was_on_stack = true;
        return;
    }

    if prompt.active.is_none() && on_stack {
        layer.stack.retain(|m| m.root != root);
        *was_on_stack = false;
        return;
    }

    if *was_on_stack && !on_stack && prompt.active.is_some() {
        prompt.active = None;
    }

    *was_on_stack = on_stack;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<ReloginPrompt>();
        app
    }

    #[test]
    fn dismiss_button_clears_prompt() {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReloginPrompt>().active = Some("KABU".to_string());
        app.add_systems(Update, relogin_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReloginDismissButton));
        app.update();
        assert!(
            app.world().resource::<ReloginPrompt>().active.is_none(),
            "閉じる must clear the relogin prompt"
        );
    }

    #[test]
    fn button_system_noop_when_closed() {
        // prompt が閉じているときに偶発的な Pressed があっても何も起きない (early return)。
        let mut app = make_app();
        app.add_systems(Update, relogin_modal_button_system);
        app.world_mut()
            .spawn((Button, Interaction::Pressed, ReloginDismissButton));
        app.update();
        assert!(app.world().resource::<ReloginPrompt>().active.is_none());
    }

    #[test]
    fn sync_writes_venue_into_info_line() {
        let mut app = make_app();
        app.world_mut().resource_mut::<ReloginPrompt>().active = Some("KABU".to_string());
        let id = app.world_mut().spawn((Text::new(""), ReloginInfoText)).id();
        app.add_systems(Update, relogin_modal_sync_system);
        app.update();
        let text = app.world().get::<Text>(id).unwrap();
        assert_eq!(text.0, "venue: KABU");
    }
}
