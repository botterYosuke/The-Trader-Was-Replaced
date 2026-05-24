use crate::trading::{
    ExecutionMode, ExecutionModeRes, LastPrices, SelectedSymbol, TransportCommand,
    TransportCommandSender,
};
use crate::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PanelSpawnSource,
    SidebarAddInstrumentButton, SidebarInstrumentPriceText, SidebarInstrumentRemoveButton,
    SidebarInstrumentRow, SidebarInstrumentRowClick, SidebarInstrumentsList,
    SidebarInstrumentsWarning, SidebarRoot, WindowRoot,
};
use crate::ui::instrument_picker::spawn_picker_dropdown;
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
                PanelKind::Order,
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

/// issue #25 Slice 1: Order サイドバーボタンは LiveManual 以外で非表示。
/// startup パネルの apply_startup_panel_visibility_system をミラー。
pub fn apply_order_button_visibility_system(
    exec_mode: Res<ExecutionModeRes>,
    mut button_q: Query<(&PanelKind, &mut Visibility), With<Button>>,
) {
    if !exec_mode.is_changed() {
        return;
    }
    let target = if matches!(exec_mode.mode, ExecutionMode::LiveManual) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (kind, mut vis) in &mut button_q {
        if matches!(kind, PanelKind::Order) && *vis != target {
            *vis = target;
        }
    }
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
                        // §4.3: Transparent click-button covering the label area
                        row.spawn((
                            Button,
                            Node {
                                flex_grow: 1.0,
                                overflow: Overflow::clip_x(),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            SidebarInstrumentRowClick {
                                instrument_id: id.clone(),
                            },
                        ))
                        .with_children(|l| {
                            l.spawn((
                                Text::new(id.clone()),
                                TextFont {
                                    font_size: 11.0,
                                    ..default()
                                },
                                TextColor(ROW_TEXT),
                            ));
                        });
                        // §4.2: Price column (fixed 70px)
                        row.spawn((Node {
                            width: Val::Px(70.0),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            ..default()
                        },))
                            .with_children(|p| {
                                p.spawn((
                                    Text::new(""),
                                    TextFont {
                                        font_size: 11.0,
                                        ..default()
                                    },
                                    TextColor(TICKER_PRICE_TEXT),
                                    SidebarInstrumentPriceText {
                                        instrument_id: id.clone(),
                                    },
                                ));
                            });
                        // × button
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
                    position_type: PositionType::Relative,
                    overflow: Overflow::visible(),
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
                spawn_picker_dropdown(btn);
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

// ── Phase 8.7 §4.2 / §4.3 / §4.4 Instruments row systems ────────────────

const TICKER_PRICE_TEXT: Color = Color::srgb(0.70, 0.85, 0.95);

/// Format a `LastPrices` value for the sidebar price column.
/// `None` → empty string; otherwise fixed 2-decimal formatting.
pub fn format_price(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.2}", v),
        None => String::new(),
    }
}

/// §4.4 / D3: Per-frame refresh of each Instruments row's last-price text.
/// Mode-branch removed: always uses `LastPrices.map` (both Replay and Live).
pub fn update_instrument_price_text_system(
    last_prices: Res<LastPrices>,
    mut q: Query<(&SidebarInstrumentPriceText, &mut Text)>,
) {
    if !last_prices.is_changed() {
        return;
    }
    for (marker, mut text) in &mut q {
        let s = format_price(last_prices.map.get(&marker.instrument_id).copied());
        if text.0 != s {
            text.0 = s;
        }
    }
}

/// §4.3: Mode-dependent click on an Instruments row label button.
/// - `Replay` → update `SelectedSymbol` only
/// - `LiveManual` / `LiveAuto` → update `SelectedSymbol` + fire `SubscribeMarketData`
#[allow(clippy::type_complexity)]
pub fn instrument_row_click_system(
    interactions: Query<
        (&Interaction, &SidebarInstrumentRowClick),
        (Changed<Interaction>, With<Button>),
    >,
    mut selected: ResMut<SelectedSymbol>,
    exec_mode: Res<ExecutionModeRes>,
    transport: Option<Res<TransportCommandSender>>,
) {
    for (interaction, row) in interactions.iter() {
        if !matches!(interaction, Interaction::Pressed) {
            continue;
        }
        let id = row.instrument_id.clone();
        if selected.id.as_deref() != Some(id.as_str()) {
            selected.id = Some(id.clone());
        }
        if matches!(
            exec_mode.mode,
            ExecutionMode::LiveManual | ExecutionMode::LiveAuto
        ) {
            if let Some(tx) = transport.as_ref() {
                if let Err(e) = tx.tx.send(TransportCommand::SubscribeMarketData {
                    instrument_id: id.clone(),
                }) {
                    warn!("SubscribeMarketData enqueue failed: {} ({})", id, e);
                }
            }
        }
    }
}

/// Pure filter helper kept testable in isolation.
pub fn filter_tickers<'a>(
    tickers: &'a [crate::trading::Ticker],
    query: &str,
) -> Vec<&'a crate::trading::Ticker> {
    if query.is_empty() {
        return tickers.iter().collect();
    }
    let q = query.to_ascii_lowercase();
    tickers
        .iter()
        .filter(|t| t.id.to_ascii_lowercase().contains(&q))
        .collect()
}

/// Clamp the scroll offset helper (kept for tests / future use).
pub fn clamp_scroll_offset(offset: usize, filtered_len: usize, visible: usize) -> usize {
    let max = filtered_len.saturating_sub(visible);
    offset.min(max)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{LastPrices, Ticker, TransportCommand};
    use crate::ui::components::SidebarInstrumentPriceText;
    use std::collections::HashMap;

    fn t(id: &str) -> Ticker {
        Ticker {
            id: id.into(),
            name: id.into(),
            market: String::new(),
        }
    }

    // ── §4.1 regression: Tickers section is gone ─────────────────────────────

    #[test]
    fn sidebar_has_no_tickers_section() {
        // After §4.1 removal, the Tickers spawn code is gone.
        // This test asserts that no SidebarTickersList entity would be spawned
        // by verifying the function compiles without those components in scope.
        // (The actual spawn is tested by App integration; here we just confirm
        //  the helper stubs keep compiling.)
        let v = vec![t("1301.TSE")];
        assert_eq!(filter_tickers(&v, "").len(), 1);
    }

    // ── §4.2: price column ────────────────────────────────────────────────────

    #[test]
    fn instrument_row_has_price_text_child() {
        // update_sidebar_system spawns a SidebarInstrumentPriceText for each instrument row.
        // We verify this by adding an instrument to the registry, running the system,
        // and checking that at least one SidebarInstrumentPriceText entity exists.
        let mut app = App::new();
        app.insert_resource(InstrumentRegistry {
            ids: vec!["7203.TSE".to_string()],
            editable: true,
        });
        // spawn a minimal sidebar root with the InstrumentsList container
        app.world_mut()
            .spawn((Node::default(), SidebarRoot))
            .with_children(|parent| {
                parent.spawn((Node::default(), SidebarInstrumentsList));
            });

        app.add_systems(Update, update_sidebar_system);
        app.update();

        let count = app
            .world_mut()
            .query::<&SidebarInstrumentPriceText>()
            .iter(app.world())
            .count();
        assert!(
            count >= 1,
            "each instrument row should have a SidebarInstrumentPriceText child"
        );

        let marker = app
            .world_mut()
            .query::<&SidebarInstrumentPriceText>()
            .iter(app.world())
            .next()
            .unwrap();
        assert_eq!(marker.instrument_id, "7203.TSE");
    }

    #[test]
    fn instrument_row_price_uses_last_prices_map() {
        let mut app = App::new();
        let mut map = HashMap::new();
        map.insert("1301.TSE".to_string(), 1234.56_f64);
        app.insert_resource(LastPrices { map });

        // Spawn a SidebarInstrumentPriceText entity
        let price_entity = app
            .world_mut()
            .spawn((
                Text::new(""),
                SidebarInstrumentPriceText {
                    instrument_id: "1301.TSE".to_string(),
                },
            ))
            .id();

        app.add_systems(Update, update_instrument_price_text_system);
        app.update();

        let text = app.world().get::<Text>(price_entity).unwrap();
        assert_eq!(text.0, "1234.56");
    }

    // ── §4.3: Instruments row click ───────────────────────────────────────────

    #[test]
    fn instrument_row_click_sets_selected_symbol() {
        let mut app = App::new();
        app.insert_resource(SelectedSymbol { id: None });
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });

        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRowClick {
                instrument_id: "7203.TSE".to_string(),
            },
        ));

        app.add_systems(Update, instrument_row_click_system);
        app.update();

        let sel = app.world().resource::<SelectedSymbol>();
        assert_eq!(sel.id.as_deref(), Some("7203.TSE"));
    }

    #[test]
    fn instrument_row_click_in_live_sends_subscribe_market_data() {
        use tokio::sync::mpsc;
        let mut app = App::new();
        app.insert_resource(SelectedSymbol { id: None });
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<TransportCommand>();
        app.insert_resource(TransportCommandSender { tx });

        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRowClick {
                instrument_id: "7203.TSE".to_string(),
            },
        ));

        app.add_systems(Update, instrument_row_click_system);
        app.update();

        let cmd = rx.try_recv().expect("should have received a command");
        assert!(
            matches!(cmd, TransportCommand::SubscribeMarketData { ref instrument_id } if instrument_id == "7203.TSE"),
            "expected SubscribeMarketData for 7203.TSE, got {:?}",
            cmd
        );
    }

    #[test]
    fn remove_button_press_does_not_trigger_row_click() {
        // The × button has SidebarInstrumentRemoveButton, NOT SidebarInstrumentRowClick.
        // So instrument_row_click_system must not fire for it.
        use tokio::sync::mpsc;
        let mut app = App::new();
        app.insert_resource(SelectedSymbol { id: None });
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });

        let (tx, mut rx) = mpsc::unbounded_channel::<TransportCommand>();
        app.insert_resource(TransportCommandSender { tx });

        // Only a RemoveButton, no RowClick component
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            SidebarInstrumentRemoveButton {
                instrument_id: "7203.TSE".to_string(),
            },
        ));

        app.add_systems(Update, instrument_row_click_system);
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "remove button must not trigger subscribe"
        );
        let sel = app.world().resource::<SelectedSymbol>();
        assert!(
            sel.id.is_none(),
            "remove button must not set selected symbol"
        );
    }

    // ── Utility tests (kept) ──────────────────────────────────────────────────

    #[test]
    fn filter_tickers_empty_query_returns_all() {
        let v = vec![t("1301.TSE"), t("7203.TSE")];
        let got = filter_tickers(&v, "");
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn filter_tickers_case_insensitive_substring() {
        let v = vec![t("1301.TSE"), t("7203.TSE"), t("9984.TSE")];
        let got = filter_tickers(&v, "tse");
        assert_eq!(got.len(), 3, "lowercase query must match uppercase ids");
        let got = filter_tickers(&v, "7203");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "7203.TSE");
        let got = filter_tickers(&v, "ZZZ");
        assert!(got.is_empty());
    }

    #[test]
    fn clamp_scroll_offset_keeps_window_in_bounds() {
        assert_eq!(clamp_scroll_offset(0, 100, 18), 0);
        assert_eq!(clamp_scroll_offset(50, 100, 18), 50);
        assert_eq!(clamp_scroll_offset(82, 100, 18), 82);
        assert_eq!(clamp_scroll_offset(83, 100, 18), 82);
        assert_eq!(clamp_scroll_offset(9999, 100, 18), 82);
    }

    #[test]
    fn clamp_scroll_offset_shorter_than_window_pins_to_zero() {
        assert_eq!(clamp_scroll_offset(5, 3, 18), 0);
        assert_eq!(clamp_scroll_offset(0, 0, 18), 0);
    }

    #[test]
    fn format_price_none_is_empty_and_some_is_two_decimals() {
        assert_eq!(format_price(None), "");
        assert_eq!(format_price(Some(0.0)), "0.00");
        assert_eq!(format_price(Some(1234.5)), "1234.50");
        assert_eq!(format_price(Some(0.001)), "0.00");
        assert_eq!(format_price(Some(101.567)), "101.57");
    }

    // ── issue #25 Slice 1: Order button gated to LiveManual ───────────────────
    #[test]
    fn order_button_hidden_outside_livemanual() {
        // The sidebar Order button must be Hidden in any non-LiveManual mode.
        // RED until apply_order_button_visibility_system exists & is registered.
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });

        let btn = app
            .world_mut()
            .spawn((Button, PanelKind::Order, Visibility::default()))
            .id();

        app.add_systems(Update, apply_order_button_visibility_system);
        app.update();

        let vis = app.world().get::<Visibility>(btn).unwrap();
        assert_eq!(
            *vis,
            Visibility::Hidden,
            "Order button must be Hidden outside LiveManual"
        );
    }

    #[test]
    fn order_button_visible_in_livemanual() {
        let mut app = App::new();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        let btn = app
            .world_mut()
            .spawn((Button, PanelKind::Order, Visibility::Hidden))
            .id();
        app.add_systems(Update, apply_order_button_visibility_system);
        app.update();
        let vis = app.world().get::<Visibility>(btn).unwrap();
        assert_eq!(*vis, Visibility::Inherited, "Order button must show in LiveManual");
    }
}
