use crate::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PanelSpawnSource,
    SidebarAddInstrumentButton, SidebarInstrumentRemoveButton, SidebarInstrumentRow,
    SidebarInstrumentsList, SidebarInstrumentsWarning, SidebarRoot, WindowRoot,
};
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

// Instrument 行用の色
const REMOVE_BTN_NORMAL: Color = Color::srgba(0.20, 0.10, 0.12, 1.0);
const REMOVE_BTN_DISABLED: Color = Color::srgba(0.15, 0.15, 0.20, 0.6);
const ROW_TEXT: Color = Color::srgb(0.80, 0.90, 1.00);
const WARNING_TEXT: Color = Color::srgb(0.95, 0.75, 0.35);

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
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                SidebarInstrumentsList,
            ));

            // ── Panels section ────────────────────────────────────────
            spawn_section_header(parent, "Panels");

            for kind in [
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

/// `InstrumentRegistry` の変更を受けて Instruments 行リストと警告行を作り直す。
pub fn update_sidebar_system(
    mut commands: Commands,
    registry: Res<InstrumentRegistry>,
    list_q: Query<Entity, With<SidebarInstrumentsList>>,
    warning_q: Query<Entity, With<SidebarInstrumentsWarning>>,
    sidebar_root_q: Query<Entity, With<SidebarRoot>>,
) {
    if !registry.is_changed() {
        return;
    }

    let Ok(list_entity) = list_q.get_single() else {
        return;
    };

    commands.entity(list_entity).despawn_descendants();

    let editable = registry.editable;
    let row_btn_bg = if editable {
        REMOVE_BTN_NORMAL
    } else {
        REMOVE_BTN_DISABLED
    };

    if registry.ids.is_empty() {
        commands.entity(list_entity).with_children(|parent| {
            parent.spawn((
                Text::new("No instruments"),
                TextFont {
                    font_size: 11.0,
                    ..default()
                },
                TextColor(Color::srgb(0.45, 0.45, 0.45)),
                Node {
                    padding: UiRect::all(Val::Px(6.0)),
                    ..default()
                },
            ));
        });
    } else {
        let ids = registry.ids.clone();
        commands.entity(list_entity).with_children(|parent| {
            for id in &ids {
                parent
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        SidebarInstrumentRow {
                            instrument_id: id.clone(),
                        },
                    ))
                    .with_children(|row| {
                        row.spawn((
                            Text::new(id.clone()),
                            TextFont {
                                font_size: 11.0,
                                ..default()
                            },
                            TextColor(ROW_TEXT),
                        ));
                        // spacer
                        row.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        row.spawn((
                            Button,
                            Node {
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                margin: UiRect::left(Val::Px(4.0)),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(row_btn_bg),
                            SidebarInstrumentRemoveButton {
                                instrument_id: id.clone(),
                            },
                        ))
                        .with_children(|btn| {
                            btn.spawn((
                                Text::new("x"),
                                TextFont {
                                    font_size: 11.0,
                                    ..default()
                                },
                                TextColor(BTN_TEXT),
                            ));
                        });
                    });
            }
        });
    }

    commands.entity(list_entity).with_children(|parent| {
        parent
            .spawn((
                Button,
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    margin: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(row_btn_bg),
                SidebarAddInstrumentButton,
            ))
            .with_children(|btn| {
                btn.spawn((
                    Text::new("+ Add"),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(BTN_TEXT),
                ));
            });
    });

    // 警告行は毎回作り直す
    for entity in warning_q.iter() {
        commands.entity(entity).despawn_recursive();
    }

    if !editable {
        if let Ok(root) = sidebar_root_q.get_single() {
            commands.entity(root).with_children(|parent| {
                parent.spawn((
                    Text::new("This sidecar uses 'instruments_ref' — read-only in Phase 7.5a"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(WARNING_TEXT),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::all(Val::Px(6.0)),
                        flex_wrap: FlexWrap::Wrap,
                        ..default()
                    },
                    SidebarInstrumentsWarning,
                ));
            });
        }
    }
}

/// Instruments 行の × ボタンを処理する。`editable=false` のときは no-op。
#[allow(clippy::type_complexity)]
pub fn instrument_remove_button_system(
    mut query: Query<
        (&Interaction, &SidebarInstrumentRemoveButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut registry: ResMut<InstrumentRegistry>,
) {
    for (interaction, btn) in &mut query {
        if matches!(interaction, Interaction::Pressed) {
            if registry.editable {
                registry.remove(&btn.instrument_id);
            }
        }
    }
}

/// パネルボタンが押されたら `PanelSpawnRequested` イベントを発火する。
/// 実際のスポーンは `panel_spawn_dispatcher_system` 側で処理。
#[allow(clippy::type_complexity)]
pub fn panel_button_system(
    mut query: Query<
        (&Interaction, &mut BackgroundColor, &PanelKind),
        (Changed<Interaction>, With<Button>),
    >,
    mut spawn_events: EventWriter<PanelSpawnRequested>,
    existing_kinds: Query<&PanelKind, With<WindowRoot>>,
) {
    for (interaction, mut bg, kind) in &mut query {
        match interaction {
            Interaction::Pressed => {
                bg.0 = BTN_PRESSED;
                // StrategyEditor は複数開けるので duplicate チェックを skip。
                // dispatcher が allocator から region_key を払い出して空エディタを生やす。
                let allow_multi = matches!(kind, PanelKind::StrategyEditor);
                if allow_multi || !existing_kinds.iter().any(|k| k == kind) {
                    spawn_events.send(PanelSpawnRequested {
                        kind: *kind,
                        source: PanelSpawnSource::User,
                        strategy_spec: None,
                    });
                }
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => bg.0 = BTN_NORMAL,
        }
    }
}
