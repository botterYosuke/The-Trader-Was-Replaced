//! Phase 7.5b — Instrument Picker module。
//!
//! 計画書 `docs/plan/Phase 7.5b - Instrument Picker.md` §2 / §3 に対応する
//! scaffolding。Resource / Component / spawn helper の宣言のみ。
//! system 実装はサブ C 以降で追加する。
//!
//! 重要:
//! - picker root には必ず `LayoutExcluded` を同時に付与すること（§3.6 / R10）。
//! - `InstrumentPickerState` は `UiPlugin::build` で `init_resource` される。

use bevy::prelude::*;
use bevy::prelude::Interaction;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::sprite::Anchor;
use chrono::NaiveDate;
use std::time::{Duration, Instant};

use crate::trading::{AvailableInstruments, TransportCommand, TransportCommandSender};
use crate::ui::components::{InstrumentRegistry, LayoutExcluded, ScenarioMetadata, SidebarAddInstrumentButton};
use crate::ui::floating_window::{FloatingWindowSpec, spawn_floating_window};

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
pub struct InstrumentPickerSearchBox;

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

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// Picker window を spawn する。
/// 既存 `spawn_floating_window` chrome に乗せ、root には `InstrumentPickerWindow` + `LayoutExcluded` を attach する
/// （§3.6 / R10: layout persistence からは除外）。
///
/// 戻り値: `(root, content_area, title_bar)`
/// - `content_area` には D-3 で searchbox、D-4 で list を子として貼る。
/// - `title_bar` は close button 追加など chrome 拡張のために返しているが、D-1 時点では未使用でよい。
pub fn spawn_picker_window(commands: &mut Commands) -> (Entity, Entity, Entity) {
    let (root, content_area, title_bar) = spawn_floating_window(
        commands,
        FloatingWindowSpec {
            title: "Add Instrument".to_string(),
            size: Vec2::new(360.0, 480.0),
            position: Vec2::new(0.0, 0.0),
            accent: Color::srgba(0.4, 0.7, 1.0, 0.8),
        },
    );
    commands
        .entity(root)
        .insert((InstrumentPickerWindow, LayoutExcluded, Name::new("InstrumentPickerWindow")));
    commands
        .spawn((
            Text2d::new(String::new()),
            TextFont { font_size: 18.0, ..default() },
            TextColor(Color::WHITE),
            Anchor::CenterLeft,
            Transform::from_xyz(-160.0, 200.0, 0.1),
            InstrumentPickerSearchBox,
            Name::new("InstrumentPickerSearchBox"),
        ))
        .set_parent(content_area);
    commands
        .spawn((
            InstrumentPickerListContainer,
            Transform::from_xyz(0.0, 0.0, 0.05),
            GlobalTransform::default(),
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
            Name::new("InstrumentPickerListContainer"),
        ))
        .set_parent(content_area);
    (root, content_area, title_bar)
}

// ---------------------------------------------------------------------------
// Systems (stub — サブ C 以降で実装)
// ---------------------------------------------------------------------------
//
// TODO(Phase 7.5b サブ C): search query update system
// TODO(Phase 7.5b サブ D): list rebuild system
// TODO(Phase 7.5b サブ D): row Add button click system
// TODO(Phase 7.5b サブ E): UiPlugin への add_systems 配線

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
    mut commands: Commands,
    mut picker: ResMut<InstrumentPickerState>,
    registry: Res<InstrumentRegistry>,
    scenario_meta: Res<ScenarioMetadata>,
    sender: Option<Res<TransportCommandSender>>,
    mut available: Option<ResMut<AvailableInstruments>>,
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
        let end_date = parse_scenario_end(&scenario_meta);
        let was_visible = picker.visible;
        picker.visible = true;
        if !was_visible {
            let _ = spawn_picker_window(&mut commands);
        }
        picker.end_date = end_date;
        picker.query.clear();
        let now = Instant::now();
        picker.last_opened_at = Some(now);

        let Some(d) = end_date else { continue; };

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
        let Some(sender) = sender.as_ref() else {
            error!("add_instrument_button_system: TransportCommandSender is None — backend not connected");
            continue;
        };

        available.in_flight.insert(d);
        let _ = sender.tx.send(TransportCommand::FetchAvailableInstruments { end_date: d });
        *last_dispatch_at = Some(now);
    }
}

/// Picker state が `visible=false` のとき、残存する `InstrumentPickerWindow` entity を despawn する。
///
/// 検出方式: state SoT を尊重し、「`!visible` かつ entity が存在する」即時条件で命中させる。
/// Query が `With<InstrumentPickerWindow>` 限定なので、despawn 後の次フレームは自然に空になり
/// 再 despawn は発生しない。前フレーム値を保持する `Local` は不要。
pub fn picker_close_when_invisible_system(
    mut commands: Commands,
    picker: Res<InstrumentPickerState>,
    windows: Query<Entity, With<InstrumentPickerWindow>>,
) {
    if picker.visible {
        return;
    }
    for entity in &windows {
        commands.entity(entity).despawn_recursive();
    }
}

/// `InstrumentPickerWindow` entity が消滅したフレームに `picker.visible = false` を立てて
/// state を SoT に再同期する。
///
/// 想定経路: 既存 `spawn_floating_window` の CloseButton observer が root を
/// `despawn_recursive` した次フレームに `Removed<InstrumentPickerWindow>` が発火する。
/// これがないと × クリック後も `picker.visible=true` のまま残り、Add ボタンの
/// debounce や搜索 input system が「見えない window」前提で走ってしまう。
///
/// 注意: `picker_close_when_invisible_system` (state→entity) とは方向が逆。
/// 両者が両立して双方向 sync を成立させる。
pub fn picker_sync_visible_on_window_removed_system(
    mut removed: RemovedComponents<InstrumentPickerWindow>,
    mut picker: ResMut<InstrumentPickerState>,
) {
    if removed.read().next().is_some() {
        if picker.visible {
            picker.visible = false;
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

/// Picker visible 中だけ KeyboardInput を読み、query を更新し searchbox Text2d に差分反映する。
/// 文字入力は `kb_events.clear()` で消費し cosmic_edit / menu_bar への二重配送を防ぐ
/// （`menu_keyboard_system` と同じパターン、§D-3-b）。
pub fn picker_searchbox_input_system(
    mut picker: ResMut<InstrumentPickerState>,
    mut kb_events: ResMut<Events<KeyboardInput>>,
    mut searchbox_q: Query<&mut Text2d, With<InstrumentPickerSearchBox>>,
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

/// 1 行 entity を spawn して container の子にする。
/// label には placeholder / error / spinner / "No matches" / instrument_id 等を渡す。
/// already_added=true のときは背景を灰色にする（ボタン無効視覚化）。
/// instrument_id が Some のときだけ Row/AddButton component を付ける。
fn spawn_picker_row(
    commands: &mut Commands,
    container: Entity,
    idx: usize,
    label: &str,
    instrument_id: Option<&str>,
    already_added: bool,
) {
    let y = 170.0 - (idx as f32) * 26.0;
    let bg = if already_added {
        Color::srgba(0.25, 0.25, 0.25, 0.6)
    } else {
        Color::srgba(0.15, 0.35, 0.55, 0.6)
    };
    let mut e = commands.spawn((
        Sprite { color: bg, custom_size: Some(Vec2::new(320.0, 24.0)), ..default() },
        Transform::from_xyz(0.0, y, 0.06),
        GlobalTransform::default(),
        Visibility::Visible,
        InheritedVisibility::default(),
        ViewVisibility::default(),
        Name::new(format!("InstrumentPickerRow#{idx}")),
    ));
    if let Some(id) = instrument_id {
        e.insert((
            InstrumentPickerRow { instrument_id: id.to_string(), already_added },
            InstrumentPickerAddButton { instrument_id: id.to_string() },
        ));
        if !already_added {
            let id_owned = id.to_string();
            e.observe(
                move |_trigger: Trigger<Pointer<Down>>,
                      mut registry: ResMut<InstrumentRegistry>,
                      mut picker: ResMut<InstrumentPickerState>| {
                    if !registry.editable {
                        return;
                    }
                    // 同一 id 100ms debounce（計画書 §3.4 / §0.4）
                    let now = Instant::now();
                    if let Some((prev_id, prev_t)) = &picker.last_added {
                        if prev_id == &id_owned && now.duration_since(*prev_t) < Duration::from_millis(100) {
                            return;
                        }
                    }
                    registry.add(&id_owned);
                    picker.last_added = Some((id_owned.clone(), now));
                    // picker は閉じない（計画書 §3.4: 連続 add 許可、close は Esc）
                },
            );
        }
    }
    let row_entity = e.id();
    commands.entity(row_entity).set_parent(container);
    commands
        .spawn((
            Text2d::new(label.to_string()),
            TextFont { font_size: 14.0, ..default() },
            TextColor(Color::WHITE),
            Anchor::CenterLeft,
            Transform::from_xyz(-150.0, 0.0, 0.01),
        ))
        .set_parent(row_entity);
}

/// Picker list を再構築する。
/// D-4-b: 実 data を描画する。trigger は picker / available / registry の変更。
pub fn picker_list_rebuild_system(
    mut commands: Commands,
    picker: Res<InstrumentPickerState>,
    available: Res<AvailableInstruments>,
    registry: Res<InstrumentRegistry>,
    container_q: Query<Entity, With<InstrumentPickerListContainer>>,
    existing_rows_q: Query<Entity, With<InstrumentPickerRow>>,
    container_children_q: Query<&Children, With<InstrumentPickerListContainer>>,
) {
    if !picker.visible {
        return;
    }
    if !(picker.is_changed() || available.is_changed() || registry.is_changed()) {
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

    // 1) end 未設定 → placeholder
    let Some(end) = picker.end_date else {
        spawn_picker_row(&mut commands, container, 0, "Set scenario.end first", None, false);
        return;
    };

    // 2) error（同 end_date の失敗のみ表示）
    if let Some((d, msg)) = &available.last_error {
        if *d == end {
            spawn_picker_row(&mut commands, container, 0, &format!("Error: {msg}"), None, false);
            return;
        }
    }

    // 3) fetch in-flight → spinner
    if available.in_flight.contains(&end) {
        spawn_picker_row(&mut commands, container, 0, "Loading...", None, false);
        return;
    }

    // 4) data
    let Some(ids) = available.by_end_date.get(&end) else {
        // request 未発火（picker_request_system 待ち）。spinner と同じ扱いにしておく。
        spawn_picker_row(&mut commands, container, 0, "Loading...", None, false);
        return;
    };

    let query_lc = picker.query.to_lowercase();
    let mut filtered: Vec<&String> = ids
        .iter()
        .filter(|id| query_lc.is_empty() || id.to_lowercase().contains(&query_lc))
        .collect();
    filtered.sort();

    if filtered.is_empty() {
        spawn_picker_row(&mut commands, container, 0, "No matches", None, false);
        return;
    }

    for (idx, id) in filtered.iter().take(15).enumerate() {
        let already = registry.contains(id);
        spawn_picker_row(&mut commands, container, idx, id, Some(id.as_str()), already);
    }
}
