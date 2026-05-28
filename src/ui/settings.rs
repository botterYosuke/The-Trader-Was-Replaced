//! Settings モーダル — on-demand spawn / close で despawn する UI Node 流派
//! (reconcile_modal / secret_modal と同系統)。

use bevy::prelude::*;

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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            GlobalZIndex(200),
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
                BackgroundColor(Color::srgba(0.07, 0.07, 0.12, 0.98)),
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
                        TextColor(Color::srgb(0.88, 0.91, 0.96)),
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
                            TextColor(Color::srgb(0.70, 0.70, 0.70)),
                        ));
                    });
                });

                // ── コンテンツ ──
                card.spawn((
                    Text::new("Theme: Dark\nBackend: localhost:19876\nSave Layout: —"),
                    TextFont { font_size: 12.0, ..default() },
                    TextColor(Color::srgb(0.75, 0.75, 0.85)),
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
) {
    let close_by_button = btn_q
        .iter()
        .any(|i| matches!(i, Interaction::Pressed));
    let close_by_escape = keys.just_pressed(KeyCode::Escape);

    if close_by_button || close_by_escape {
        for entity in &root_q {
            commands.entity(entity).despawn();
        }
    }
}
