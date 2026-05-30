//! Settings モーダル — on-demand spawn / close で despawn する UI Node 流派
//! (reconcile_modal / secret_modal と同系統)。

use bevy::prelude::*;
use crate::trading::SecretPrompt;
use crate::ui::theme::Theme;
use crate::ui::modify_modal::ModifyForm;
use crate::ui::order_panel::OrderConfirm;

// ────────────────────────────────────────────────
// Components
// ────────────────────────────────────────────────

#[derive(Component)]
pub struct SettingsModalRoot;

#[derive(Component)]
pub struct SettingsCloseButton;

// ────────────────────────────────────────────────
// Spawn
// ────────────────────────────────────────────────

pub fn spawn_settings_modal(commands: &mut Commands) {
    let theme = Theme::default();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::BLACK.with_alpha(0.5)),
            GlobalZIndex(195),
            SettingsModalRoot,
            Name::new("SettingsModal"),
        ))
        .with_children(|p| {
            // ── カード ──
            p.spawn((
                Node {
                    width: Val::Px(320.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    ..default()
                },
                BackgroundColor(theme.colors.surface_background),
            ))
            .with_children(|card| {
                // ── ヘッダ行: "Settings" (左) + × ボタン (右) ──
                card.spawn((Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(12.0)),
                    width: Val::Percent(100.0),
                    ..default()
                },))
                .with_children(|row| {
                    row.spawn((
                        Text::new("Settings"),
                        TextFont { font_size: 14.0, ..default() },
                        TextColor(theme.colors.text),
                    ));
                    row.spawn((
                        Button,
                        Node {
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                        SettingsCloseButton,
                    ))
                    .with_children(|btn| {
                        btn.spawn((
                            Text::new("×"),
                            TextFont { font_size: 14.0, ..default() },
                            TextColor(theme.colors.text_muted),
                        ));
                    });
                });

                // ── コンテンツ ──
                card.spawn((
                    Text::new("Theme: Dark\nBackend: localhost:19876\nSave Layout: —"),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(theme.colors.text_muted),
                ));
            });
        });
}

// ────────────────────────────────────────────────
// Close system
// ────────────────────────────────────────────────

pub fn settings_modal_close_system(
    mut commands: Commands,
    btn_q: Query<&Interaction, (Changed<Interaction>, With<SettingsCloseButton>)>,
    root_q: Query<Entity, With<SettingsModalRoot>>,
    keys: Res<ButtonInput<KeyCode>>,
    secret_prompt: Res<SecretPrompt>,
    order_confirm: Res<OrderConfirm>,
    modify_form: Res<ModifyForm>,
) {
    let close_by_button = btn_q
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    // 高優先モーダルが開いている間は Escape を yield する（§3.10）。
    let higher_priority_open =
        secret_prompt.active.is_some() || order_confirm.pending.is_some() || modify_form.open;
    let close_by_escape = keys.just_pressed(KeyCode::Escape) && !higher_priority_open;

    if close_by_button || close_by_escape {
        for entity in &root_q {
            commands.entity(entity).despawn();
        }
    }
}
