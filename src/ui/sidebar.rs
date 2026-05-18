use crate::trading::{
    ExecutionMode, ExecutionModeRes, LastPrices, SelectedSymbol, Tickers, TradingData,
    TransportCommand, TransportCommandSender,
};
use crate::ui::components::{
    InstrumentRegistry, PanelKind, PanelSpawnRequested, PanelSpawnSource,
    SidebarAddInstrumentButton, SidebarInstrumentRemoveButton, SidebarInstrumentRow,
    SidebarInstrumentsList, SidebarInstrumentsWarning, SidebarRoot, SidebarTickerPriceText,
    SidebarTickerRow, SidebarTickersList, SidebarTickersScrollOffset, SidebarTickersSearchBox,
    SidebarTickersSearchState, SidebarTickersSearchText, WindowRoot,
};
use crate::ui::instrument_picker::spawn_picker_dropdown;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

/// Number of Ticker rows kept spawned at any given time. The list is
/// virtualized so that browsing thousands of instruments does not balloon
/// the entity count.
const TICKERS_VISIBLE_ROWS: usize = 18;
const TICKER_ROW_HEIGHT_PX: f32 = 18.0;

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

            // ── Tickers (Live universe) section — Phase 8 §3.5 ────────
            spawn_section_header(parent, "Tickers");

            // Search box. Click to focus; while focused, keyboard input is
            // drained by `tickers_search_input_system`.
            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                        margin: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BTN_NORMAL),
                    SidebarTickersSearchBox,
                ))
                .with_children(|btn| {
                    btn.spawn((
                        Text::new("Search…"),
                        TextFont { font_size: 11.0, ..default() },
                        TextColor(Color::srgb(0.55, 0.60, 0.72)),
                        SidebarTickersSearchText,
                    ));
                });

            // Virtual-scroll viewport. Fixed height = visible_rows * row_height
            // so the spawn count is bounded regardless of universe size.
            parent.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(
                        TICKER_ROW_HEIGHT_PX * TICKERS_VISIBLE_ROWS as f32,
                    ),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(2.0)),
                    overflow: Overflow::clip_y(),
                    ..default()
                },
                Interaction::default(),
                SidebarTickersList,
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

// ── Phase 8 §3.5 sidebar Tickers (Live universe) ─────────────────────────

const TICKER_ROW_NORMAL: Color = Color::srgba(0.08, 0.08, 0.13, 1.0);
const TICKER_ROW_SELECTED: Color = Color::srgba(0.20, 0.32, 0.50, 1.0);
const TICKER_PRICE_TEXT: Color = Color::srgb(0.70, 0.85, 0.95);

/// Pure filter helper kept testable in isolation. Case-insensitive substring
/// match against id, with empty query meaning "all".
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

/// Clamp the scroll offset so the visible window never falls off the end of
/// the filtered list.
pub fn clamp_scroll_offset(offset: usize, filtered_len: usize, visible: usize) -> usize {
    let max = filtered_len.saturating_sub(visible);
    offset.min(max)
}

/// Rebuild the visible row slice whenever the universe, filter, or scroll
/// offset changes. We despawn the full container's children each time —
/// `TICKERS_VISIBLE_ROWS` rows is bounded, so the cost is independent of
/// the universe size.
pub fn update_tickers_list_system(
    mut commands: Commands,
    tickers: Res<Tickers>,
    search: Res<SidebarTickersSearchState>,
    mut scroll: ResMut<SidebarTickersScrollOffset>,
    selected: Res<SelectedSymbol>,
    list_q: Query<Entity, With<SidebarTickersList>>,
) {
    if !(tickers.is_changed()
        || search.is_changed()
        || scroll.is_changed()
        || selected.is_changed())
    {
        return;
    }
    let Ok(list_entity) = list_q.get_single() else {
        return;
    };

    let filtered = filter_tickers(&tickers.list, &search.query);
    let clamped = clamp_scroll_offset(scroll.first_visible, filtered.len(), TICKERS_VISIBLE_ROWS);
    // Write back only if it actually shifted, to avoid retriggering on the
    // next frame via Res<>::is_changed().
    if scroll.first_visible != clamped {
        scroll.first_visible = clamped;
    }

    commands.entity(list_entity).despawn_descendants();

    if filtered.is_empty() {
        commands.entity(list_entity).with_children(|parent| {
            let msg = if search.query.is_empty() {
                "No instruments"
            } else {
                "No matches"
            };
            parent.spawn((
                Text::new(msg),
                TextFont { font_size: 11.0, ..default() },
                TextColor(Color::srgb(0.45, 0.45, 0.45)),
                Node { padding: UiRect::all(Val::Px(6.0)), ..default() },
            ));
        });
        return;
    }

    let end = (clamped + TICKERS_VISIBLE_ROWS).min(filtered.len());
    let slice: Vec<(String, String)> = filtered[clamped..end]
        .iter()
        .map(|t| (t.id.clone(), t.name.clone()))
        .collect();
    let selected_id = selected.id.clone();
    commands.entity(list_entity).with_children(|parent| {
        for (id, name) in slice {
            let is_selected = selected_id.as_deref() == Some(id.as_str());
            let bg = if is_selected {
                TICKER_ROW_SELECTED
            } else {
                TICKER_ROW_NORMAL
            };
            parent
                .spawn((
                    Button,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(TICKER_ROW_HEIGHT_PX),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(0.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(bg),
                    SidebarTickerRow { instrument_id: id.clone() },
                ))
                .with_children(|row| {
                    // Show name when distinct from id (e.g. once Live venue
                    // adapters fill the field); otherwise just the id.
                    let label = if name.is_empty() || name == id {
                        id.clone()
                    } else {
                        format!("{}  {}", id, name)
                    };
                    // Label (left-aligned, takes remaining horizontal space).
                    row.spawn((
                        Node {
                            flex_grow: 1.0,
                            overflow: Overflow::clip_x(),
                            ..default()
                        },
                    ))
                    .with_children(|l| {
                        l.spawn((
                            Text::new(label),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(ROW_TEXT),
                        ));
                    });
                    // Price column (right-aligned, fixed 70px).
                    row.spawn((
                        Node {
                            width: Val::Px(70.0),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ))
                    .with_children(|p| {
                        p.spawn((
                            Text::new(""),
                            TextFont { font_size: 11.0, ..default() },
                            TextColor(TICKER_PRICE_TEXT),
                            SidebarTickerPriceText { instrument_id: id.clone() },
                        ));
                    });
                });
        }
    });
}

/// Toggle search-box focus on click. Click on any other UI elsewhere does
/// NOT unfocus — Escape (handled by the input drain) clears + unfocuses.
pub fn tickers_search_focus_system(
    mut interactions: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>, With<SidebarTickersSearchBox>),
    >,
    mut search: ResMut<SidebarTickersSearchState>,
) {
    for (interaction, mut bg) in &mut interactions {
        match interaction {
            Interaction::Pressed => {
                search.focused = true;
                bg.0 = BTN_PRESSED;
            }
            Interaction::Hovered => bg.0 = BTN_HOVER,
            Interaction::None => {
                bg.0 = if search.focused { BTN_PRESSED } else { BTN_NORMAL };
            }
        }
    }
}

/// Drain keyboard input into the search query while the search box is
/// focused. Mirrors the pattern in `picker_searchbox_input_system` so that
/// menu_bar / cosmic_edit do not double-consume the same key events.
pub fn tickers_search_input_system(
    mut search: ResMut<SidebarTickersSearchState>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
) {
    if !search.focused {
        return;
    }
    for ev in kb_events.drain() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for ch in s.chars() {
                    if !ch.is_control() {
                        search.query.push(ch);
                    }
                }
            }
            Key::Backspace => {
                search.query.pop();
            }
            Key::Space => search.query.push(' '),
            Key::Escape => {
                search.query.clear();
                search.focused = false;
            }
            _ => {}
        }
    }
}

/// Mirror the search query to the visible Text node, with placeholder
/// behavior when the query is empty.
pub fn tickers_search_text_sync_system(
    search: Res<SidebarTickersSearchState>,
    mut text_q: Query<(&mut Text, &mut TextColor), With<SidebarTickersSearchText>>,
) {
    if !search.is_changed() {
        return;
    }
    let Ok((mut text, mut color)) = text_q.get_single_mut() else {
        return;
    };
    let (display, c) = if search.query.is_empty() {
        ("Search…".to_string(), Color::srgb(0.55, 0.60, 0.72))
    } else {
        (search.query.clone(), Color::srgb(0.85, 0.90, 1.00))
    };
    if text.0 != display {
        text.0 = display;
    }
    if color.0 != c {
        color.0 = c;
    }
}

/// Format a `LastPrices` / `TradingData.close` value for the sidebar price
/// column. `None` → empty string so the column visually clears; otherwise
/// fixed 2-decimal formatting (matches the sub-yen tick granularity of the
/// venues we currently target).
pub fn format_price(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.2}", v),
        None => String::new(),
    }
}

/// Per-frame refresh of each ticker row's last-price text. Writes are
/// guarded so unchanged rows do not retrigger Bevy's change-detection.
/// - Live*: `LastPrices.map.get(&id)` → format
/// - Replay: `Some(TradingData.close)` only when `SelectedSymbol.id == id`,
///   otherwise empty
pub fn update_ticker_price_text_system(
    exec_mode: Res<ExecutionModeRes>,
    last_prices: Res<LastPrices>,
    selected: Res<SelectedSymbol>,
    trading: Res<TradingData>,
    mut q: Query<(&SidebarTickerPriceText, &mut Text)>,
) {
    let is_replay = matches!(exec_mode.mode, ExecutionMode::Replay);
    let selected_id = selected.id.as_deref();
    for (marker, mut text) in &mut q {
        let value: Option<f64> = if is_replay {
            if selected_id == Some(marker.instrument_id.as_str()) {
                trading.close.map(|c| c as f64)
            } else {
                None
            }
        } else {
            last_prices.map.get(&marker.instrument_id).copied()
        };
        let s = format_price(value);
        if text.0 != s {
            text.0 = s;
        }
    }
}

/// Mouse-wheel scrolling over the Tickers viewport. One notch advances by
/// 3 rows for keyboards/wheels and by the raw `y` for trackpads.
pub fn tickers_scroll_system(
    mut wheel: EventReader<MouseWheel>,
    mut scroll: ResMut<SidebarTickersScrollOffset>,
    tickers: Res<Tickers>,
    search: Res<SidebarTickersSearchState>,
    viewport_q: Query<&Interaction, With<SidebarTickersList>>,
) {
    let hovered = viewport_q
        .get_single()
        .map(|i| matches!(i, Interaction::Hovered | Interaction::Pressed))
        .unwrap_or(false);
    if !hovered {
        wheel.clear();
        return;
    }
    let mut delta_rows: i32 = 0;
    for ev in wheel.read() {
        let step = match ev.unit {
            MouseScrollUnit::Line => -ev.y.round() as i32 * 3,
            MouseScrollUnit::Pixel => -(ev.y / TICKER_ROW_HEIGHT_PX).round() as i32,
        };
        delta_rows += step;
    }
    if delta_rows == 0 {
        return;
    }
    let filtered_len = filter_tickers(&tickers.list, &search.query).len();
    let cur = scroll.first_visible as i32;
    let max = filtered_len.saturating_sub(TICKERS_VISIBLE_ROWS) as i32;
    let next = (cur + delta_rows).clamp(0, max);
    if next != cur {
        scroll.first_visible = next as usize;
    }
}

/// Mode-dependent click on a Ticker row.
/// - `Replay` → update `SelectedSymbol` only (plan §3.5 1009)
/// - `LiveManual` / `LiveAuto` → update `SelectedSymbol` + fire
///   `SubscribeMarketData` (plan §3.5 1010)
#[allow(clippy::type_complexity)]
pub fn ticker_row_click_system(
    interactions: Query<
        (&Interaction, &SidebarTickerRow),
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
    use crate::trading::Ticker;

    fn t(id: &str) -> Ticker {
        Ticker { id: id.into(), name: id.into(), market: String::new() }
    }

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
        // 100 items, visible=18 → max offset 82.
        assert_eq!(clamp_scroll_offset(0, 100, 18), 0);
        assert_eq!(clamp_scroll_offset(50, 100, 18), 50);
        assert_eq!(clamp_scroll_offset(82, 100, 18), 82);
        assert_eq!(clamp_scroll_offset(83, 100, 18), 82);
        assert_eq!(clamp_scroll_offset(9999, 100, 18), 82);
    }

    #[test]
    fn clamp_scroll_offset_shorter_than_window_pins_to_zero() {
        // Universe smaller than the visible window → always 0.
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
}
