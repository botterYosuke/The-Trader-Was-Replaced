use crate::trading::InstrumentList;
use crate::ui::components::{PanelKind, PanelSpawnRequested, SidebarListLabel, SidebarRoot};
use bevy::prelude::*;

const SIDEBAR_WIDTH: f32 = 180.0;
const FOOTER_HEIGHT: f32 = 28.0;
const MENU_BAR_HEIGHT: f32 = 24.0;

const BG: Color = Color::srgba(0.05, 0.05, 0.09, 0.95);
const SECTION_HEADER_BG: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BORDER: Color = Color::srgba(0.18, 0.18, 0.28, 1.0);

// パネルボタンの状態色（menu_bar.rs と同値、Step 1 完了後に共通化検討）
const BTN_NORMAL: Color = Color::srgba(0.10, 0.10, 0.16, 1.0);
const BTN_HOVER: Color = Color::srgba(0.20, 0.20, 0.30, 1.0);
const BTN_PRESSED: Color = Color::srgba(0.30, 0.30, 0.48, 1.0);
const BTN_TEXT: Color = Color::srgb(0.78, 0.82, 0.92);

pub fn spawn_sidebar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(MENU_BAR_HEIGHT),
                left: Val::Px(0.0),
                bottom: Val::Px(FOOTER_HEIGHT),
                width: Val::Px(SIDEBAR_WIDTH),
                flex_direction: FlexDirection::Column,
                border: UiRect::right(Val::Px(1.0)),
                overflow: Overflow::clip_y(),
                ..default()
            },
            BackgroundColor(BG),
            BorderColor(BORDER),
            SidebarRoot,
        ))
        .with_children(|parent| {
            // ── Instruments section ───────────────────────────────────
            spawn_section_header(parent, "Instruments");

            parent.spawn((
                Text::new("Loading…"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.75, 0.70, 0.25)),
                Node {
                    padding: UiRect::all(Val::Px(6.0)),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                },
                SidebarListLabel,
            ));

            // ── Panels section ────────────────────────────────────────
            spawn_section_header(parent, "Panels");

            for kind in [
                PanelKind::Chart,
                PanelKind::StrategyEditor,
                PanelKind::BuyingPower,
                PanelKind::RunResult,
                PanelKind::Positions,
                PanelKind::Orders,
            ] {
                spawn_panel_btn(parent, kind);
            }

            // ── Settings stub ─────────────────────────────────────────
            parent.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });

            spawn_section_header(parent, "Settings");

            parent.spawn((
                Text::new("Theme: Dark\nBackend: localhost:19876\nSave Layout: —"),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.45, 0.45, 0.55)),
                Node {
                    padding: UiRect::all(Val::Px(6.0)),
                    ..default()
                },
            ));
        });
}

fn spawn_section_header(parent: &mut ChildBuilder, title: &str) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(SECTION_HEADER_BG),
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(title),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.50, 0.70, 1.00)),
            ));
        });
}

fn spawn_panel_btn(parent: &mut ChildBuilder, kind: PanelKind) {
    parent
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                margin: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(BTN_NORMAL),
            kind, // PanelKind 自身をマーカーとして付ける
        ))
        .with_children(|p| {
            p.spawn((
                Text::new(kind.label()),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(BTN_TEXT),
            ));
        });
}

pub fn update_sidebar_system(
    list: Res<InstrumentList>,
    mut label_q: Query<(&mut Text, &mut TextColor), With<SidebarListLabel>>,
) {
    if !list.is_changed() {
        return;
    }

    let Ok((mut text, mut color)) = label_q.get_single_mut() else {
        return;
    };

    if !list.loaded {
        text.0 = "Loading…".to_string();
        color.0 = Color::srgb(0.75, 0.70, 0.25);
        return;
    }

    if let Some(err) = &list.error {
        text.0 = format!("Error:\n{}", err);
        color.0 = Color::srgb(1.00, 0.28, 0.28);
        return;
    }

    if list.ids.is_empty() {
        text.0 = "No instruments".to_string();
        color.0 = Color::srgb(0.45, 0.45, 0.45);
        return;
    }

    text.0 = list.ids.join("\n");
    color.0 = Color::srgb(0.80, 0.90, 1.00);
}

/// パネルボタンが押されたら `PanelSpawnRequested` イベントを発火する。
/// 実際のスポーンは `panel_spawn_dispatcher_system` 側で処理。
pub fn panel_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &PanelKind),
        (Changed<Interaction>, With<Button>),
    >,
    mut spawn_events: EventWriter<PanelSpawnRequested>,
) {
    for (interaction, mut bg, kind) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                spawn_events.send(PanelSpawnRequested { kind: *kind });
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}
