//! Phase 7.5b — Instrument Picker module。
//!
//! 計画書 `docs/plan/Phase 7.5b - Instrument Picker.md` §2 / §3 に対応する
//! scaffolding。Resource / Component / spawn helper の宣言のみ。
//! system 実装はサブ C 以降で追加する。
//!
//! 重要:
//! - picker root には必ず `LayoutExcluded` を同時に付与すること（§3.6 / R10）。
//! - `InstrumentPickerState` は `UiPlugin::build` で `init_resource` される。

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::Interaction;
use bevy::prelude::*;
use chrono::NaiveDate;
use std::time::{Duration, Instant};

use crate::trading::{
    AvailableInstruments, BackendStatus, ExecutionMode, ExecutionModeRes, Tickers, TickersSource,
    TickersStatus, TransportCommand, TransportCommandSender, VenueState, VenueStatusRes,
    is_live_mode, is_venue_live,
};
use crate::ui::components::{InstrumentRegistry, ScenarioMetadata, SidebarAddInstrumentButton};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

#[derive(Resource, Debug, Default, Clone)]
pub struct InstrumentPickerState {
    pub visible: bool,
    pub end_date: Option<NaiveDate>,
    pub query: String,
    pub last_opened_at: Option<Instant>,
    pub last_added: Option<(String, Instant)>,
}

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerWindow;

#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerListContainer;

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerRow {
    pub instrument_id: String,
    pub already_added: bool,
}

#[derive(Component, Debug, Clone)]
pub struct InstrumentPickerAddButton {
    pub instrument_id: String,
}

/// Dropdown popup Node 自身の marker。`[+ Add]` ボタン直下に spawn される。
/// `sync_picker_dropdown_visibility_system` が `picker.visible` に応じて Display を切り替える。
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerDropdown;

/// 検索クエリを表示する UI `Text` の marker。
/// `picker_searchbox_input_system` が `picker.query` を差分書き込みする。
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct InstrumentPickerSearchText;

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// `[+ Add]` ボタン直下に spawn する dropdown popup Node。
///
/// `menu_bar.rs` の File popup と同じ流派:
/// - `display: Display::None` で start し、`sync_picker_dropdown_visibility_system`
///   が `picker.visible` に応じて Flex/None を切り替える (後続手で追加)。
/// - `position_type: Absolute` で親 (= Add ボタン Node) の **右** (`left: 100%`)、
///   上端揃え (`top: 0`) に配置。menu_bar は下に開くが、Sidebar 内なので右に出す。
/// - `GlobalZIndex(100)` で他 UI より前面に。
///
/// 子構成:
/// - 上段: `InstrumentPickerSearchText` を持つ `Text` (現 query 表示)。
/// - 下段: `InstrumentPickerListContainer` を持つ Node (行 Button を後で picker_list_rebuild_system が spawn)。
///
/// 注意: 呼び出し側 (sidebar の Add ボタン spawn 内) は次手で配線する。
/// 現時点では未配線なので `#[allow(dead_code)]`。
pub fn spawn_picker_dropdown(parent: &mut ChildBuilder) {
    parent
        .spawn((
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                left: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(240.0),
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.10, 0.10, 0.16, 0.98)),
            GlobalZIndex(100),
            InstrumentPickerDropdown,
            Name::new("InstrumentPickerDropdown"),
        ))
        .with_children(|p| {
            // 上段: 検索クエリ表示 Text (差分書き込み対象)
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                    margin: UiRect::bottom(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.06, 0.10, 1.0)),
            ))
            .with_children(|sb| {
                sb.spawn((
                    Text::new(String::new()),
                    TextFont {
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    InstrumentPickerSearchText,
                    Name::new("InstrumentPickerSearchText"),
                ));
            });

            // 下段: 行 Button の親 container
            p.spawn((
                Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                InstrumentPickerListContainer,
                Name::new("InstrumentPickerListContainer"),
            ));
        });
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// `ScenarioMetadata.end` (Option<String>) を NaiveDate に parse する。
/// 未設定 / parse 失敗時は None（picker は placeholder で open する想定）。
pub(crate) fn parse_scenario_end(meta: &ScenarioMetadata) -> Option<NaiveDate> {
    meta.end
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
}

/// Sidebar `[+ Add]` 押下で picker を open する system。
/// C-2-c スコープ: open 処理 + 100ms debounce 付きで
/// `TransportCommand::FetchAvailableInstruments` を発行する。
pub fn add_instrument_button_system(
    mut picker: ResMut<InstrumentPickerState>,
    registry: Res<InstrumentRegistry>,
    scenario_meta: Res<ScenarioMetadata>,
    exec_mode: Res<ExecutionModeRes>,
    sender: Option<Res<TransportCommandSender>>,
    backend_status: Option<Res<BackendStatus>>,
    mut available: Option<ResMut<AvailableInstruments>>,
    tickers: Option<Res<Tickers>>,
    mut last_dispatch_at: Local<Option<Instant>>,
    interactions: Query<&Interaction, (Changed<Interaction>, With<SidebarAddInstrumentButton>)>,
) {
    if !registry.editable {
        return;
    }
    for interaction in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let was_visible = picker.visible;
        if was_visible {
            picker.visible = false;
            continue;
        }
        picker.visible = true;
        picker.query.clear();
        let now = Instant::now();
        picker.last_opened_at = Some(now);

        match exec_mode.mode {
            ExecutionMode::Replay => {
                let end_date = parse_scenario_end(&scenario_meta);
                picker.end_date = end_date;

                let Some(d) = end_date else {
                    continue;
                };

                let time_ok = match *last_dispatch_at {
                    None => true,
                    Some(prev) => now.duration_since(prev) >= Duration::from_millis(100),
                };
                if !time_ok {
                    continue;
                }

                let Some(available) = available.as_mut() else {
                    error!("add_instrument_button_system: AvailableInstruments resource missing");
                    continue;
                };
                if available.by_end_date.contains_key(&d) || available.in_flight.contains(&d) {
                    continue;
                }
                if backend_status
                    .as_ref()
                    .map(|s| !s.connected)
                    .unwrap_or(true)
                {
                    available.last_error = Some((d, "backend not connected".to_string()));
                    continue;
                }
                let Some(sender) = sender.as_ref() else {
                    error!(
                        "add_instrument_button_system: TransportCommandSender is None — backend not connected"
                    );
                    available.last_error = Some((d, "backend transport unavailable".to_string()));
                    continue;
                };
                available.in_flight.insert(d);
                available.last_error = None;
                let _ = sender
                    .tx
                    .send(TransportCommand::FetchAvailableInstruments { end_date: d });
                *last_dispatch_at = Some(now);
            }
            ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
                picker.end_date = None;
                // §4.6: only re-fetch when NotFetched or Failed
                let should_fetch = tickers
                    .as_ref()
                    .map(|t| {
                        matches!(
                            t.status,
                            TickersStatus::NotFetched | TickersStatus::Failed(_)
                        )
                    })
                    .unwrap_or(true);
                if should_fetch {
                    if let Some(tx) = sender.as_ref() {
                        let _ = tx.tx.send(TransportCommand::ListInstruments {
                            source: TickersSource::LiveVenue,
                        });
                    }
                }
            }
        }
    }
}

/// `[+ Add]` ボタン直下の dropdown popup の Display を `picker.visible` に同期する。
///
/// trigger 条件:
/// - `picker.is_changed()`: visible トグルを拾う通常経路。
/// - `!added_dropdown_q.is_empty()`: registry 変更で sidebar list descendants が
///   despawn → 新しい Add ボタンと共に dropdown が再 spawn された直後フレーム。
///   この場合 `picker.is_changed()` は立たないので Added<...> 側で命中させる。
///
/// 動作: `picker.visible` に応じて `node.display = Flex / None`。
pub fn sync_picker_dropdown_visibility_system(
    picker: Res<InstrumentPickerState>,
    added_dropdown_q: Query<(), Added<InstrumentPickerDropdown>>,
    mut dropdown_q: Query<&mut Node, With<InstrumentPickerDropdown>>,
) {
    if !(picker.is_changed() || !added_dropdown_q.is_empty()) {
        return;
    }
    let target = if picker.visible {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut dropdown_q {
        if node.display != target {
            node.display = target;
        }
    }
}

/// Picker dropdown 内の row (`[+ Add]` 候補) クリックを拾い、
/// `handle_picker_row_click` に委譲する production system。
///
/// `spawn_picker_row_ui` で spawn された Button 群が対象。
/// Bevy の `Changed<Interaction>` filter により Pressed エッジのみハンドル。
pub fn picker_row_click_system(
    interactions: Query<
        (&Interaction, &InstrumentPickerAddButton),
        (Changed<Interaction>, With<Button>),
    >,
    mut registry: ResMut<InstrumentRegistry>,
    mut picker: ResMut<InstrumentPickerState>,
) {
    for (interaction, btn) in interactions.iter() {
        if matches!(interaction, Interaction::Pressed) {
            handle_picker_row_click(
                &btn.instrument_id,
                &mut registry,
                &mut picker,
                Instant::now(),
            );
        }
    }
}

/// `registry.editable == false` の間は picker を強制 close し、`available.last_error` も
/// reset する（計画書 Phase 7.5b §3.5 / §3.7）。
pub fn force_close_picker_on_lock_system(
    registry: Res<InstrumentRegistry>,
    mut picker: ResMut<InstrumentPickerState>,
    mut available: Option<ResMut<AvailableInstruments>>,
) {
    if registry.editable {
        return;
    }
    if !picker.visible {
        return;
    }
    picker.visible = false;
    picker.query.clear();
    if let Some(av) = available.as_mut() {
        av.last_error = None;
    }
}

/// Picker visible 中だけ KeyboardInput を読み、query を更新し searchbox UI Text に差分反映する。
/// 文字入力は `kb_events.clear()` で消費し cosmic_edit / menu_bar への二重配送を防ぐ
/// （`menu_keyboard_system` と同じパターン、§D-3-b）。
pub fn picker_searchbox_input_system(
    mut picker: ResMut<InstrumentPickerState>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
    mut searchbox_q: Query<&mut Text, With<InstrumentPickerSearchText>>,
) {
    if !picker.visible {
        return;
    }
    let mut consumed = false;
    let mut changed = false;
    // drain して読む（後段への配送を止めるため）
    for ev in kb_events.drain() {
        if !ev.state.is_pressed() {
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) => {
                for ch in s.chars() {
                    if !ch.is_control() {
                        picker.query.push(ch);
                        changed = true;
                    }
                }
                consumed = true;
            }
            Key::Backspace => {
                if picker.query.pop().is_some() {
                    changed = true;
                }
                consumed = true;
            }
            Key::Escape => {
                picker.visible = false;
                picker.query.clear();
                changed = true;
                consumed = true;
            }
            Key::Space => {
                picker.query.push(' ');
                changed = true;
                consumed = true;
            }
            _ => {}
        }
    }
    let _ = consumed; // drain 自体が消費。明示変数は将来 modifier 対応の足場
    if changed {
        if let Ok(mut text) = searchbox_q.get_single_mut() {
            if text.0 != picker.query {
                text.0 = picker.query.clone();
            }
        }
    }
}

/// UI Node 版の picker 行 spawn。Sprite/Text2d 版 `spawn_picker_row` の置換候補。
/// 行 entity は Button + Node で、container (Node, FlexDirection::Column) の子になる。
/// クリックは observer ではなく `picker_row_click_system` が Interaction 経由で処理する。
#[allow(dead_code)]
fn spawn_picker_row_ui(
    commands: &mut Commands,
    container: Entity,
    idx: usize,
    label: &str,
    instrument_id: Option<&str>,
    already_added: bool,
) {
    let bg = if already_added {
        Color::srgba(0.25, 0.25, 0.25, 0.6)
    } else {
        Color::srgba(0.15, 0.35, 0.55, 0.6)
    };
    let mut e = commands.spawn((
        Button,
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(24.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(0.0)),
            margin: UiRect::bottom(Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexStart,
            ..default()
        },
        BackgroundColor(bg),
        Name::new(format!("InstrumentPickerRow#{idx}")),
    ));
    if let Some(id) = instrument_id {
        e.insert((
            InstrumentPickerRow {
                instrument_id: id.to_string(),
                already_added,
            },
            InstrumentPickerAddButton {
                instrument_id: id.to_string(),
            },
        ));
    }
    let row_entity = e.id();
    commands.entity(row_entity).set_parent(container);
    commands
        .spawn((
            Text::new(label.to_string()),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::WHITE),
        ))
        .set_parent(row_entity);
}

/// Picker row click を処理する純粋ハンドラ。
/// 同一 id 100ms debounce（計画書 §3.4 / §0.4）。picker は閉じない（連続 add 許可、close は Esc）。
fn handle_picker_row_click(
    id: &str,
    registry: &mut InstrumentRegistry,
    picker: &mut InstrumentPickerState,
    now: Instant,
) {
    if !registry.editable {
        return;
    }
    if let Some((prev_id, prev_t)) = &picker.last_added {
        if prev_id == id && now.duration_since(*prev_t) < Duration::from_millis(100) {
            return;
        }
    }
    registry.add(id);
    picker.last_added = Some((id.to_string(), now));
}

/// Picker list を再構築する。
/// §4.5: ExecutionMode で分岐 — Replay → AvailableInstruments、Live → Tickers.list。
pub fn picker_list_rebuild_system(
    mut commands: Commands,
    picker: Res<InstrumentPickerState>,
    available: Res<AvailableInstruments>,
    registry: Res<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    tickers: Option<Res<Tickers>>,
    container_q: Query<Entity, With<InstrumentPickerListContainer>>,
    added_container_q: Query<(), Added<InstrumentPickerListContainer>>,
    existing_rows_q: Query<Entity, With<InstrumentPickerRow>>,
    container_children_q: Query<&Children, With<InstrumentPickerListContainer>>,
) {
    if !picker.visible {
        return;
    }
    // 再 open で container が同フレームに spawn された場合、Res の changed flag は
    // 前フレームのものなので落ち得る。container 新規生成自体を rebuild trigger に含める。
    let container_added = !added_container_q.is_empty();
    let tickers_changed = tickers.as_ref().map(|t| t.is_changed()).unwrap_or(false);
    if !(container_added
        || picker.is_changed()
        || available.is_changed()
        || registry.is_changed()
        || exec_mode.is_changed()
        || tickers_changed)
    {
        return;
    }
    let Ok(container) = container_q.get_single() else {
        return;
    };

    // 既存の子（行 + placeholder 行 全部）を despawn
    if let Ok(children) = container_children_q.get_single() {
        for &child in children.iter() {
            commands.entity(child).despawn_recursive();
        }
    }
    // 念のため orphan の Row も掃除
    for entity in &existing_rows_q {
        commands.entity(entity).despawn_recursive();
    }

    // §4.5: mode 分岐で候補 ids を確定する
    let ids: Vec<String> = match exec_mode.mode {
        ExecutionMode::Replay => {
            // Replay 経路: AvailableInstruments から end_date キーで引く
            // 1) end 未設定 → placeholder
            let Some(end) = picker.end_date else {
                spawn_picker_row_ui(
                    &mut commands,
                    container,
                    0,
                    "Set scenario.end first",
                    None,
                    false,
                );
                return;
            };

            // 2) error（同 end_date の失敗のみ表示）
            if let Some((d, msg)) = &available.last_error {
                if *d == end {
                    spawn_picker_row_ui(
                        &mut commands,
                        container,
                        0,
                        &format!("Error: {msg}"),
                        None,
                        false,
                    );
                    return;
                }
            }

            // 3) fetch in-flight → spinner
            if available.in_flight.contains(&end) {
                spawn_picker_row_ui(&mut commands, container, 0, "Loading...", None, false);
                return;
            }

            // 4) data
            let Some(raw_ids) = available.by_end_date.get(&end) else {
                // request 未発火（picker_request_system 待ち）。spinner と同じ扱いにしておく。
                spawn_picker_row_ui(&mut commands, container, 0, "Loading...", None, false);
                return;
            };

            raw_ids.iter().cloned().collect()
        }
        ExecutionMode::LiveManual | ExecutionMode::LiveAuto => {
            // Live 経路: Tickers.status に応じて placeholder / list
            match tickers.as_ref().map(|t| &t.status) {
                None | Some(TickersStatus::NotFetched) => {
                    spawn_picker_row_ui(
                        &mut commands,
                        container,
                        0,
                        "Venue not connected",
                        None,
                        false,
                    );
                    return;
                }
                Some(TickersStatus::InFlight) => {
                    spawn_picker_row_ui(&mut commands, container, 0, "Loading...", None, false);
                    return;
                }
                Some(TickersStatus::Failed(msg)) => {
                    let msg = msg.clone();
                    spawn_picker_row_ui(
                        &mut commands,
                        container,
                        0,
                        &format!("Error: {msg}"),
                        None,
                        false,
                    );
                    return;
                }
                Some(TickersStatus::Loaded) => tickers
                    .as_ref()
                    .map(|t| t.list.iter().map(|tk| tk.id.clone()).collect())
                    .unwrap_or_default(),
            }
        }
    };

    // 共通: filter + sort + spawn rows
    let query_lc = picker.query.to_lowercase();
    let mut filtered: Vec<&String> = ids
        .iter()
        .filter(|id| query_lc.is_empty() || id.to_lowercase().contains(&query_lc))
        .collect();
    filtered.sort();

    if filtered.is_empty() {
        spawn_picker_row_ui(&mut commands, container, 0, "No matches", None, false);
        return;
    }

    for (idx, id) in filtered.iter().take(15).enumerate() {
        let already = registry.contains(id);
        spawn_picker_row_ui(
            &mut commands,
            container,
            idx,
            id,
            Some(id.as_str()),
            already,
        );
    }
}

/// §4.6.1: VenueState → Connected/Subscribed 遷移 OR ExecutionMode → Live* 遷移で
/// live universe を auto-fetch する（D23 拡張）。
/// Note: UiPlugin への登録は Round 3 wire-up で行う。
pub fn auto_fetch_live_universe_on_connect_system(
    venue: Res<VenueStatusRes>,
    exec_mode: Res<ExecutionModeRes>,
    tickers: Res<Tickers>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_state: Local<Option<VenueState>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_venue = venue.state;
    let was_venue = prev_state.replace(cur_venue);
    let cur_mode = exec_mode.mode;
    let was_mode = prev_mode.replace(cur_mode);

    let became_connected = is_venue_live(cur_venue) && !was_venue.is_some_and(is_venue_live);
    let became_live = is_live_mode(cur_mode) && !was_mode.is_some_and(is_live_mode);

    // Trigger 1: VenueState が live になった瞬間（Live mode 前提）
    let trigger_by_venue = became_connected && is_live_mode(cur_mode);
    // Trigger 2: ExecutionMode が Live になった瞬間（VenueState すでに live 前提）
    let trigger_by_mode = became_live && is_venue_live(cur_venue);

    if !(trigger_by_venue || trigger_by_mode) {
        return;
    }
    // Tickers がすでに Loaded ならスキップ（重複 fetch 防止）。InFlight も race するので skip。
    if matches!(
        tickers.status,
        TickersStatus::Loaded | TickersStatus::InFlight
    ) {
        return;
    }
    if let Some(tx) = sender.as_ref() {
        let _ = tx.tx.send(TransportCommand::ListInstruments {
            source: TickersSource::LiveVenue,
        });
    }
}

/// §4.6.2: Replay 入場 / scenario.end 変更で AvailableInstruments を auto-fetch する（D11）。
/// Note: UiPlugin への登録は Round 3 wire-up で行う。
pub fn auto_fetch_available_on_replay_entry_system(
    exec_mode: Res<ExecutionModeRes>,
    scenario: Res<ScenarioMetadata>,
    sender: Option<Res<TransportCommandSender>>,
    mut available: ResMut<AvailableInstruments>,
    backend_status: Option<Res<BackendStatus>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
    mut prev_end: Local<Option<String>>,
) {
    let cur_mode = exec_mode.mode;
    let cur_end = scenario.end.clone();
    let mode_entered_replay = prev_mode.replace(cur_mode) != Some(ExecutionMode::Replay)
        && cur_mode == ExecutionMode::Replay;
    let end_changed = *prev_end != cur_end;
    if end_changed {
        *prev_end = cur_end.clone();
    }
    if !mode_entered_replay && !end_changed {
        return;
    }
    if cur_mode != ExecutionMode::Replay {
        return;
    }
    let Some(end) = parse_scenario_end(&scenario) else {
        return;
    };
    if available.by_end_date.contains_key(&end) || available.in_flight.contains(&end) {
        return;
    }
    if backend_status
        .as_ref()
        .map(|s| !s.connected)
        .unwrap_or(true)
    {
        available.last_error = Some((end, "backend not connected".to_string()));
        return;
    }
    let Some(tx) = sender.as_ref() else {
        return;
    };
    available.in_flight.insert(end);
    available.last_error = None;
    let _ = tx
        .tx
        .send(TransportCommand::FetchAvailableInstruments { end_date: end });
}

/// §4.3.1 / D22: registry に追加された instrument を Live mode で自動 subscribe する。
/// mode 切替直後 frame は survivor を bulk subscribe する（Replay → Live 半接続防止）。
/// Note: UiPlugin への登録は Round 3 wire-up で行う。
pub fn subscribe_added_instruments_system(
    registry: Res<InstrumentRegistry>,
    exec_mode: Res<ExecutionModeRes>,
    sender: Option<Res<TransportCommandSender>>,
    mut prev_ids: Local<HashSet<String>>,
    mut prev_mode: Local<Option<ExecutionMode>>,
) {
    let cur_mode = exec_mode.mode;
    let mode_changed = prev_mode.replace(cur_mode) != Some(cur_mode);
    // `current` always built to keep `prev_ids` accurate for future mode switches.
    // Clones of `prev_ids` (for bulk path) and `added` Vec are deferred until needed.
    let current: HashSet<String> = registry.ids.iter().cloned().collect();
    if !is_live_mode(cur_mode) {
        *prev_ids = current;
        return;
    }
    let Some(tx) = sender.as_ref() else {
        *prev_ids = current;
        return;
    };
    // mode 切替直後 frame: prev_ids に残っている survivor を bulk subscribe（上限 50）。
    if mode_changed {
        const BULK_SUBSCRIBE_CAP: usize = 50;
        let survivors: Vec<String> = prev_ids
            .intersection(&current)
            .take(BULK_SUBSCRIBE_CAP)
            .cloned()
            .collect();
        *prev_ids = current;
        for id in survivors {
            let _ = tx
                .tx
                .send(TransportCommand::SubscribeMarketData { instrument_id: id });
        }
        return;
    }
    let added: Vec<String> = current.difference(&prev_ids).cloned().collect();
    *prev_ids = current;
    for id in added {
        let _ = tx
            .tx
            .send(TransportCommand::SubscribeMarketData { instrument_id: id });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::{
        AvailableInstruments, ExecutionMode, ExecutionModeRes, Tickers, TickersSource,
        TickersStatus, VenueState, VenueStatusRes,
    };
    use crate::ui::components::{InstrumentRegistry, ScenarioMetadata, SidebarAddInstrumentButton};
    use chrono::NaiveDate;
    use tokio::sync::mpsc as tokio_mpsc;

    fn make_transport() -> (
        TransportCommandSender,
        tokio_mpsc::UnboundedReceiver<TransportCommand>,
    ) {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        (TransportCommandSender { tx }, rx)
    }

    fn make_app() -> App {
        let mut app = App::new();
        app.init_resource::<InstrumentPickerState>();
        app.insert_resource(InstrumentRegistry {
            ids: vec![],
            editable: true,
        });
        app.insert_resource(ScenarioMetadata {
            schema_version: None,
            instruments: vec![],
            start: None,
            end: Some("2024-12-31".to_string()),
            granularity: None,
            initial_cash: None,
        });
        app.insert_resource(AvailableInstruments::default());
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.insert_resource(Tickers::default());
        app
    }

    #[test]
    fn test_picker_opens_on_add_button_pressed() {
        let mut app = make_app();
        let btn = app
            .world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed))
            .id();
        app.add_systems(Update, add_instrument_button_system);
        app.update();

        let picker = app.world().resource::<InstrumentPickerState>();
        assert!(picker.visible, "Add button press should open picker");
        assert_eq!(
            picker.end_date,
            Some(NaiveDate::from_ymd_opt(2024, 12, 31).unwrap()),
            "end_date snapshot should be taken from ScenarioMetadata.end"
        );
        let _ = btn;
    }

    #[test]
    fn test_picker_skips_open_when_registry_locked() {
        let mut app = make_app();
        app.insert_resource(InstrumentRegistry {
            ids: vec![],
            editable: false,
        });
        app.world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed));
        app.add_systems(Update, add_instrument_button_system);
        app.update();

        let picker = app.world().resource::<InstrumentPickerState>();
        assert!(!picker.visible, "locked registry must not open picker");
    }

    #[test]
    fn test_picker_skips_open_during_debounce() {
        let mut app = make_app();
        let d = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .in_flight
            .insert(d);

        let btn = app
            .world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed))
            .id();
        app.add_systems(Update, add_instrument_button_system);
        app.update();
        *app.world_mut().get_mut::<Interaction>(btn).unwrap() = Interaction::None;
        app.update();
        *app.world_mut().get_mut::<Interaction>(btn).unwrap() = Interaction::Pressed;
        app.update();

        let av = app.world().resource::<AvailableInstruments>();
        assert_eq!(
            av.in_flight.len(),
            1,
            "in_flight must not double-register on rapid re-press"
        );
    }

    #[test]
    fn test_picker_force_close_on_lock() {
        let mut app = make_app();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .visible = true;
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .query = "abc".to_string();
        app.insert_resource(InstrumentRegistry {
            ids: vec![],
            editable: false,
        });

        app.add_systems(Update, force_close_picker_on_lock_system);
        app.update();

        let picker = app.world().resource::<InstrumentPickerState>();
        assert!(!picker.visible, "lock must force-close picker");
        assert!(picker.query.is_empty(), "lock must clear query");
    }

    // ─── 5-a-2: handle_picker_row_click 単体テスト ──────────────────────

    #[test]
    fn test_picker_click_adds_to_registry() {
        let mut registry = InstrumentRegistry {
            ids: vec![],
            editable: true,
        };
        let mut picker = InstrumentPickerState::default();
        let now = Instant::now();

        handle_picker_row_click("7203", &mut registry, &mut picker, now);

        assert!(registry.contains("7203"));
        assert_eq!(registry.ids, vec!["7203".to_string()]);
        assert_eq!(picker.last_added, Some(("7203".to_string(), now)));
    }

    #[test]
    fn test_picker_click_is_idempotent_for_already_added() {
        let mut registry = InstrumentRegistry {
            ids: vec!["7203".to_string()],
            editable: true,
        };
        let mut picker = InstrumentPickerState::default();
        let now = Instant::now();

        handle_picker_row_click("7203", &mut registry, &mut picker, now);

        assert_eq!(registry.ids, vec!["7203".to_string()]);
        assert_eq!(picker.last_added, Some(("7203".to_string(), now)));
    }

    #[test]
    fn test_picker_click_debounces_same_id_only() {
        let mut registry = InstrumentRegistry {
            ids: vec![],
            editable: true,
        };
        let mut picker = InstrumentPickerState::default();
        let t0 = Instant::now();

        handle_picker_row_click("7203", &mut registry, &mut picker, t0);
        assert_eq!(registry.ids, vec!["7203".to_string()]);
        let after_first = picker.last_added.clone();

        let t1 = t0 + Duration::from_millis(50);
        handle_picker_row_click("7203", &mut registry, &mut picker, t1);
        assert_eq!(registry.ids, vec!["7203".to_string()]);
        assert_eq!(picker.last_added, after_first, "last_added は更新されない");

        handle_picker_row_click("9984", &mut registry, &mut picker, t1);
        assert_eq!(registry.ids, vec!["7203".to_string(), "9984".to_string()]);
        assert_eq!(picker.last_added, Some(("9984".to_string(), t1)));
    }

    #[test]
    fn test_picker_click_blocked_when_locked() {
        let mut registry = InstrumentRegistry {
            ids: vec![],
            editable: false,
        };
        let mut picker = InstrumentPickerState::default();
        let now = Instant::now();

        handle_picker_row_click("7203", &mut registry, &mut picker, now);

        assert!(registry.ids.is_empty());
        assert_eq!(picker.last_added, None);
    }

    /// §5.4 項目 7 回帰 pin: backend disconnect 時に [+ Add] を押すと
    /// in_flight に入れずに last_error をセットし、picker は spinner 永久ループにならない。
    #[test]
    fn test_picker_sets_last_error_when_backend_disconnected() {
        use crate::trading::BackendStatus;
        let mut app = make_app();
        app.insert_resource(BackendStatus {
            connected: false,
            running: false,
            last_error: None,
        });

        app.world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed));
        app.add_systems(Update, add_instrument_button_system);
        app.update();

        let d = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        let av = app.world().resource::<AvailableInstruments>();
        assert!(
            !av.in_flight.contains(&d),
            "disconnect 時は in_flight に入れない"
        );
        assert_eq!(
            av.last_error.as_ref().map(|(date, _)| *date),
            Some(d),
            "disconnect 時は last_error を当該 end_date でセット",
        );
    }

    /// §5.4 項目 5-a 回帰 pin (dropdown 版): picker close → 再 open で
    /// dropdown の Display が None → Flex に追従すること。world-space window 廃止後は
    /// list 再構築自体は sidebar 配下の container 存在に依存するため、
    /// ここでは visible トグル → Display sync のラウンドトリップのみを pin する。
    #[test]
    fn test_picker_list_rebuilds_on_reopen_after_close() {
        let mut app = make_app();

        // sidebar 全体を spawn せず、dropdown entity だけを直接 spawn する。
        // 初期 Display は spawn_picker_dropdown と同じ Display::None。
        let dropdown = app
            .world_mut()
            .spawn((
                Node {
                    display: Display::None,
                    ..Default::default()
                },
                InstrumentPickerDropdown,
            ))
            .id();

        app.add_systems(
            Update,
            (
                add_instrument_button_system,
                sync_picker_dropdown_visibility_system,
            )
                .chain(),
        );

        // 1) 初回 open: Add ボタン押下 → picker.visible = true → Display::Flex
        let btn = app
            .world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed))
            .id();
        app.update();
        *app.world_mut().get_mut::<Interaction>(btn).unwrap() = Interaction::None;
        app.update();
        {
            let node = app.world().get::<Node>(dropdown).unwrap();
            assert_eq!(
                node.display,
                Display::Flex,
                "初回 open で dropdown が Flex 表示になる"
            );
        }

        // 2) close: visible = false → Display::None
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .visible = false;
        app.update();
        {
            let node = app.world().get::<Node>(dropdown).unwrap();
            assert_eq!(
                node.display,
                Display::None,
                "close で dropdown が None に戻る"
            );
        }

        // 3) 再 open: 再 Pressed → Display::Flex
        *app.world_mut().get_mut::<Interaction>(btn).unwrap() = Interaction::Pressed;
        app.update();
        *app.world_mut().get_mut::<Interaction>(btn).unwrap() = Interaction::None;
        app.update();

        let node = app.world().get::<Node>(dropdown).unwrap();
        assert_eq!(
            node.display,
            Display::Flex,
            "再 open で dropdown が再び Flex 表示になる (regression pin)"
        );
    }

    // ─── Step 10: §4.5 picker_list_rebuild_system mode-branch ────────────

    fn make_list_app_with_container() -> (App, Entity) {
        let mut app = make_app();
        app.add_systems(Update, picker_list_rebuild_system);
        // picker is visible
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .visible = true;
        // spawn a minimal InstrumentPickerListContainer
        let container = app
            .world_mut()
            .spawn((Node::default(), InstrumentPickerListContainer))
            .id();
        (app, container)
    }

    #[test]
    fn picker_dropdown_uses_available_in_replay_mode() {
        let (mut app, _container) = make_list_app_with_container();
        // set up end_date and available instruments
        let d = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        app.world_mut()
            .resource_mut::<InstrumentPickerState>()
            .end_date = Some(d);
        app.world_mut()
            .resource_mut::<AvailableInstruments>()
            .by_end_date
            .insert(d, vec!["7203".to_string(), "9984".to_string()]);
        // ExecutionMode::Replay (default in make_app)
        app.update();
        let count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(count, 2, "Replay mode: rows from AvailableInstruments");
    }

    #[test]
    fn picker_dropdown_uses_tickers_in_live_mode() {
        let (mut app, _container) = make_list_app_with_container();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![
                crate::trading::Ticker {
                    id: "7203.T".to_string(),
                    name: "".to_string(),
                    market: "T".to_string(),
                },
                crate::trading::Ticker {
                    id: "9984.T".to_string(),
                    name: "".to_string(),
                    market: "T".to_string(),
                },
            ],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Loaded,
        });
        app.update();
        let count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(count, 2, "Live mode: rows from Tickers.list");
    }

    #[test]
    fn picker_dropdown_shows_not_fetched_placeholder_in_live() {
        let (mut app, container) = make_list_app_with_container();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::Unknown,
            status: TickersStatus::NotFetched,
        });
        app.update();
        // no InstrumentPickerRow (just placeholder text child)
        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "NotFetched: no picker rows");
        // container should have a child (placeholder button)
        let children_count = app
            .world()
            .get::<Children>(container)
            .map(|c| c.len())
            .unwrap_or(0);
        assert!(children_count > 0, "NotFetched: placeholder spawned");
    }

    #[test]
    fn picker_dropdown_shows_in_flight_placeholder_in_live() {
        let (mut app, container) = make_list_app_with_container();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::InFlight,
        });
        app.update();
        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "InFlight: no picker rows");
        let children_count = app
            .world()
            .get::<Children>(container)
            .map(|c| c.len())
            .unwrap_or(0);
        assert!(children_count > 0, "InFlight: loading placeholder spawned");
    }

    #[test]
    fn picker_dropdown_shows_failed_placeholder_in_live() {
        let (mut app, container) = make_list_app_with_container();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Failed("timeout".to_string()),
        });
        app.update();
        let row_count = app
            .world_mut()
            .query::<&InstrumentPickerRow>()
            .iter(app.world())
            .count();
        assert_eq!(row_count, 0, "Failed: no picker rows");
        let children_count = app
            .world()
            .get::<Children>(container)
            .map(|c| c.len())
            .unwrap_or(0);
        assert!(children_count > 0, "Failed: error placeholder spawned");
    }

    #[test]
    fn add_button_in_live_does_not_fetch_available_or_require_scenario_end() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        // No scenario end
        app.insert_resource(ScenarioMetadata {
            schema_version: None,
            instruments: vec![],
            start: None,
            end: None,
            granularity: None,
            initial_cash: None,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Loaded,
        });
        let (transport, _rx) = make_transport();
        app.insert_resource(transport);

        app.world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed));
        app.add_systems(Update, add_instrument_button_system);
        app.update();

        let picker = app.world().resource::<InstrumentPickerState>();
        assert!(
            picker.visible,
            "Live mode: picker opens even without scenario end"
        );
        assert_eq!(picker.end_date, None, "Live mode: end_date remains None");
        // AvailableInstruments should NOT have been touched
        let av = app.world().resource::<AvailableInstruments>();
        assert!(
            av.in_flight.is_empty(),
            "Live mode: no FetchAvailableInstruments fired"
        );
    }

    #[test]
    fn add_button_in_live_triggers_list_instruments_live_when_not_fetched() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::Unknown,
            status: TickersStatus::NotFetched,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);

        app.world_mut()
            .spawn((SidebarAddInstrumentButton, Interaction::Pressed));
        app.add_systems(Update, add_instrument_button_system);
        app.update();

        let picker = app.world().resource::<InstrumentPickerState>();
        assert!(picker.visible, "picker opens");
        // should have sent ListInstruments
        let cmd = rx.try_recv().expect("ListInstruments should be sent");
        assert!(
            matches!(
                cmd,
                TransportCommand::ListInstruments {
                    source: TickersSource::LiveVenue
                }
            ),
            "should send ListInstruments with LiveVenue source, got {:?}",
            cmd
        );
    }

    // ─── Step 10: §4.6.1 auto_fetch_live_universe_on_connect_system ──────

    #[test]
    fn venue_connected_transition_auto_fetches_live_universe() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        // Start disconnected
        app.insert_resource(VenueStatusRes {
            state: VenueState::Disconnected,
            ..Default::default()
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_live_universe_on_connect_system);

        // Frame 1: disconnected, records prev_state=Disconnected, prev_mode=LiveManual
        app.update();
        assert!(rx.try_recv().is_err(), "disconnected: no fetch");

        // Frame 2: transition to Connected
        app.insert_resource(VenueStatusRes {
            state: VenueState::Connected,
            ..Default::default()
        });
        app.update();

        let cmd = rx
            .try_recv()
            .expect("Connected transition should trigger fetch");
        assert!(
            matches!(
                cmd,
                TransportCommand::ListInstruments {
                    source: TickersSource::LiveVenue
                }
            ),
            "should send ListInstruments LiveVenue"
        );
    }

    #[test]
    fn venue_connected_transition_does_not_double_fetch() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(VenueStatusRes {
            state: VenueState::Disconnected,
            ..Default::default()
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_live_universe_on_connect_system);

        // Frame 1: record initial state
        app.update();

        // Frame 2: Connected → triggers fetch, mark Tickers as InFlight
        app.insert_resource(VenueStatusRes {
            state: VenueState::Connected,
            ..Default::default()
        });
        app.insert_resource(Tickers {
            list: vec![],
            source: TickersSource::LiveVenue,
            status: TickersStatus::InFlight,
        });
        app.update();
        let _ = rx.try_recv(); // consume one (may or may not fire due to InFlight guard)

        // Frame 3: still Connected — no new fetch
        app.update();
        assert!(
            rx.try_recv().is_err(),
            "should not double-fetch when already InFlight/Connected"
        );
    }

    #[test]
    fn mode_entered_live_with_venue_already_connected_fetches_live_universe() {
        let mut app = make_app();
        // Start in Replay with Connected venue
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.insert_resource(VenueStatusRes {
            state: VenueState::Connected,
            ..Default::default()
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_live_universe_on_connect_system);

        // Frame 1: Replay + Connected, records initial state
        app.update();
        assert!(rx.try_recv().is_err(), "Replay mode: no fetch");

        // Frame 2: switch to Live (venue already Connected)
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.update();

        let cmd = rx
            .try_recv()
            .expect("mode→Live with Connected venue should trigger fetch");
        assert!(
            matches!(
                cmd,
                TransportCommand::ListInstruments {
                    source: TickersSource::LiveVenue
                }
            ),
            "should send ListInstruments LiveVenue"
        );
    }

    #[test]
    fn mode_entered_live_skips_fetch_when_tickers_already_loaded() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.insert_resource(VenueStatusRes {
            state: VenueState::Connected,
            ..Default::default()
        });
        app.insert_resource(Tickers {
            list: vec![crate::trading::Ticker {
                id: "7203.T".to_string(),
                name: "".to_string(),
                market: "T".to_string(),
            }],
            source: TickersSource::LiveVenue,
            status: TickersStatus::Loaded,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_live_universe_on_connect_system);

        app.update(); // record initial

        // Switch to Live — but Tickers already Loaded, should skip
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "Tickers Loaded: should not re-fetch"
        );
    }

    // ─── Step 10: §4.6.2 auto_fetch_available_on_replay_entry_system ─────

    #[test]
    fn replay_entry_auto_fetches_available_instruments() {
        let mut app = make_app();
        // Start in Live mode
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.insert_resource(crate::trading::BackendStatus {
            connected: true,
            running: true,
            last_error: None,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_available_on_replay_entry_system);

        app.update(); // record initial Live mode
        assert!(rx.try_recv().is_err(), "Live mode: no fetch");

        // Switch to Replay
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.update();

        let cmd = rx
            .try_recv()
            .expect("Replay entry should trigger FetchAvailableInstruments");
        let d = NaiveDate::from_ymd_opt(2024, 12, 31).unwrap();
        assert!(
            matches!(cmd, TransportCommand::FetchAvailableInstruments { end_date } if end_date == d),
            "should fetch available with end=2024-12-31, got {:?}",
            cmd
        );
    }

    #[test]
    fn scenario_end_change_in_replay_refetches_available_instruments() {
        let mut app = make_app();
        // Start in Replay already (no mode transition, but end changes)
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.insert_resource(crate::trading::BackendStatus {
            connected: true,
            running: true,
            last_error: None,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_available_on_replay_entry_system);

        // Frame 1: initial Replay, end=2024-12-31 — will trigger as prev_end=None != Some("2024-12-31")
        app.update();
        let _ = rx.try_recv(); // consume initial fetch

        // Frame 2: change end date
        app.world_mut().resource_mut::<ScenarioMetadata>().end = Some("2025-03-31".to_string());
        app.update();

        let cmd = rx.try_recv().expect("end change in Replay should re-fetch");
        let d2 = NaiveDate::from_ymd_opt(2025, 3, 31).unwrap();
        assert!(
            matches!(cmd, TransportCommand::FetchAvailableInstruments { end_date } if end_date == d2),
            "should fetch available with new end=2025-03-31, got {:?}",
            cmd
        );
    }

    #[test]
    fn replay_startup_fetches_local_universe() {
        // Regression pin: on first startup in Replay (prev_end=None → changed), fetch fires
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::Replay,
        });
        app.insert_resource(crate::trading::BackendStatus {
            connected: true,
            running: true,
            last_error: None,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, auto_fetch_available_on_replay_entry_system);

        app.update();

        let cmd = rx
            .try_recv()
            .expect("startup Replay should fetch available instruments");
        assert!(
            matches!(cmd, TransportCommand::FetchAvailableInstruments { .. }),
            "should be FetchAvailableInstruments, got {:?}",
            cmd
        );
    }

    // ─── Step 10: D22 subscribe_added_instruments_system ─────────────────

    #[test]
    fn subscribe_sent_for_added_id_in_live() {
        let mut app = make_app();
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, subscribe_added_instruments_system);

        // Frame 1: empty registry, records prev_ids={}, prev_mode=LiveManual
        app.update();
        let _ = rx.try_recv(); // discard any bulk subscribe on first frame

        // Frame 2: add an instrument
        app.world_mut()
            .resource_mut::<InstrumentRegistry>()
            .ids
            .push("7203.T".to_string());
        app.update();

        let cmd = rx
            .try_recv()
            .expect("added id in Live should send SubscribeMarketData");
        assert!(
            matches!(&cmd, TransportCommand::SubscribeMarketData { instrument_id } if instrument_id == "7203.T"),
            "should subscribe 7203.T, got {:?}",
            cmd
        );
    }

    #[test]
    fn subscribe_not_sent_in_replay() {
        let mut app = make_app();
        // Replay mode (default)
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, subscribe_added_instruments_system);

        app.update();
        app.world_mut()
            .resource_mut::<InstrumentRegistry>()
            .ids
            .push("7203.T".to_string());
        app.update();

        assert!(
            rx.try_recv().is_err(),
            "Replay mode: no SubscribeMarketData sent"
        );
    }

    #[test]
    fn subscribe_not_sent_on_mode_change_frame() {
        let mut app = make_app();
        // Start Replay with an existing instrument
        app.world_mut()
            .resource_mut::<InstrumentRegistry>()
            .ids
            .push("7203.T".to_string());
        let (transport, mut rx) = make_transport();
        app.insert_resource(transport);
        app.add_systems(Update, subscribe_added_instruments_system);

        // Frame 1: Replay — records prev_ids={"7203.T"}, prev_mode=Replay
        app.update();
        assert!(rx.try_recv().is_err(), "Replay: no subscribe");

        // Frame 2: switch to Live — mode_changed=true, goes to bulk path (survivors)
        // survivors = prev_for_bulk ∩ current = {"7203.T"} — bulk IS sent (that's correct per plan)
        // The test pin is that the NORMAL diff path does not fire a second time
        app.insert_resource(ExecutionModeRes {
            mode: ExecutionMode::LiveManual,
        });
        app.update();
        // consume the bulk subscribe if any
        let _ = rx.try_recv();
        // no more commands
        assert!(
            rx.try_recv().is_err(),
            "mode-change frame: only bulk (or nothing), no extra diff subscribe"
        );
    }
}
