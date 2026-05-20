use crate::ui::buying_power::spawn_buying_power_panel;
use crate::ui::components::{
    ChartInstrument, CloseButton, InstrumentRegistry, PanelKind, PanelSpawnRequested,
    PanelSpawnSource, PendingStrategyFragments, RegionKeyAllocator, StrategyBuffer,
    StrategyEditorId, StrategyEditorSpawnSpec, StrategyFragment, TitleBar, WindowManager,
    WindowRoot,
};
use crate::ui::editor_history::{ActiveDrag, AppHistory};
use crate::ui::layout_persistence::WindowLayout;
use crate::ui::menu_bar::cache_state_paths;
use crate::ui::orders::spawn_orders_panel;
use crate::ui::positions::spawn_positions_panel;
use crate::ui::run_result_panel::spawn_run_result_panel;
use crate::ui::strategy_editor::spawn_strategy_editor_panel;
use bevy::prelude::*;
use bevy_cosmic_edit::prelude::CosmicFontSystem;

/// floating window の title bar 高さ。chart レイアウト定数 (`chart_viewstate.rs`) もこれを参照する
/// (Caveat #33: 二重定義すると chart の draw 領域が枠を ~8px はみ出す)。
pub const TITLE_BAR_HEIGHT: f32 = 40.0;

/// floating window を生成するときに渡す設定。
#[derive(Clone)]
pub struct FloatingWindowSpec {
    /// タイトルバーに表示する文字列
    pub title: String,
    /// ウィンドウの幅 (x) と高さ (y)、ピクセル単位
    pub size: Vec2,
    /// 画面（world-space）上の初期位置
    pub position: Vec2,
    /// rim light（外周の発光）の色
    pub accent: Color,
}

/// 戻り値: (root_entity, content_area_entity, title_bar_entity)
/// - root_entity: ウィンドウ全体の親。位置を動かしたいときはこれを動かす
/// - content_area_entity: タイトルバーの下の領域。中身（チャート・テキストなど）はここの子にする
/// - title_bar_entity: タイトルバー sprite。タイトル右端にボタンを足したい panel 用に公開する
pub fn spawn_floating_window(
    commands: &mut Commands,
    spec: FloatingWindowSpec,
) -> (Entity, Entity, Entity) {
    const TITLE_PADDING_LEFT: f32 = 16.0;
    let title_bar_half = TITLE_BAR_HEIGHT / 2.0;

    // ─── 1. Window root (背景) ───
    let root = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.07, 0.07, 0.12, 0.85),
                custom_size: Some(spec.size),
                ..default()
            },
            Transform::from_xyz(spec.position.x, spec.position.y, 10.0),
            WindowRoot,
        ))
        .observe(
            |trigger: Trigger<Pointer<Down>>,
             mut query: Query<&mut Transform, With<WindowRoot>>,
             mut wm: ResMut<WindowManager>| {
                wm.max_z += 2.0;
                if let Ok(mut transform) = query.get_mut(trigger.entity()) {
                    transform.translation.z = 10.0 + wm.max_z;
                }
            },
        )
        .id();

    // ─── 2. Inner glow (内側のうっすら白い光) ───
    commands
        .spawn((
            Sprite {
                color: Color::srgba(1.0, 1.0, 1.0, 0.05),
                custom_size: Some(spec.size - Vec2::splat(4.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.01),
        ))
        .set_parent(root);

    // ─── 3. Rim light (外周の色付き発光、accent 色を使う) ───
    commands
        .spawn((
            Sprite {
                color: spec.accent,
                // rim は両側に 1px ずつはみ出す → 幅・高さ +2
                custom_size: Some(spec.size + Vec2::splat(2.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -0.01),
        ))
        .set_parent(root);

    // ─── 4. Title bar (上端のドラッグ可能なバー) ───
    let title_bar_y = spec.size.y / 2.0 - title_bar_half;
    let title_bar = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.1, 0.1, 0.2, 1.0),
                custom_size: Some(Vec2::new(spec.size.x, TITLE_BAR_HEIGHT)),
                ..default()
            },
            Transform::from_xyz(0.0, title_bar_y, 0.1),
            TitleBar,
        ))
        .observe(
            |drag: Trigger<Pointer<Drag>>,
             mut query: Query<&mut Transform, With<WindowRoot>>,
             parent_query: Query<&Parent>,
             camera_query: Query<&OrthographicProjection, With<Camera2d>>| {
                let Ok(parent) = parent_query.get(drag.entity()) else {
                    return;
                };
                let Ok(mut transform) = query.get_mut(parent.get()) else {
                    return;
                };
                let scale = camera_query.get_single().map(|p| p.scale).unwrap_or(1.0);
                transform.translation.x += drag.event().delta.x * scale;
                transform.translation.y -= drag.event().delta.y * scale;
            },
        )
        .observe(
            |drag_start: Trigger<Pointer<DragStart>>,
             parent_query: Query<&Parent>,
             root_q: Query<&Transform, With<WindowRoot>>,
             mut active_drag: ResMut<ActiveDrag>| {
                let Ok(parent) = parent_query.get(drag_start.entity()) else {
                    return;
                };
                let root_entity = parent.get();
                let Ok(tf) = root_q.get(root_entity) else {
                    return;
                };
                active_drag
                    .starts
                    .insert(root_entity, tf.translation.truncate());
            },
        )
        .observe(
            |drag_end: Trigger<Pointer<DragEnd>>,
             parent_query: Query<&Parent>,
             root_q: Query<
                (
                    &Transform,
                    &PanelKind,
                    Option<&StrategyEditorId>,
                    Option<&ChartInstrument>,
                ),
                With<WindowRoot>,
            >,
             mut active_drag: ResMut<ActiveDrag>,
             mut history: ResMut<AppHistory>,
             mut auto_save: ResMut<crate::ui::layout_persistence::AutoSaveState>| {
                let Ok(parent) = parent_query.get(drag_end.entity()) else {
                    return;
                };
                let root_entity = parent.get();
                let Some(before) = active_drag.starts.remove(&root_entity) else {
                    return;
                };
                let Ok((tf, kind, editor_id, chart_instrument)) = root_q.get(root_entity) else {
                    return;
                };
                if chart_instrument.is_some() {
                    return;
                }
                let after = tf.translation.truncate();
                let region_key = editor_id.map(|id| id.region_key.clone());
                history.push_window_move(*kind, region_key, before, after);
                auto_save.mark_layout_changed(std::time::Instant::now());
            },
        )
        .id();
    commands.entity(root).add_child(title_bar);

    // ─── 5. Title text (タイトルバーに乗る文字) ───
    let title_text_x = -spec.size.x / 2.0 + TITLE_PADDING_LEFT;
    let title_text = commands
        .spawn((
            Text2d::new(spec.title.clone()),
            TextFont {
                font_size: 20.0,
                ..default()
            },
            TextColor(Color::WHITE),
            bevy::sprite::Anchor::CenterLeft,
            Transform::from_xyz(title_text_x, 0.0, 0.1),
        ))
        .id();
    commands.entity(title_bar).add_child(title_text);

    // ─── 6. Content area (中身を載せる場所。タイトルバーの下) ───
    let content_area = commands
        .spawn(Transform::from_xyz(0.0, -title_bar_half, 0.1))
        .id();
    commands.entity(root).add_child(content_area);

    // ─── 7. Close button (× — タイトルバー右端。root 直下に置くことで
    //        title_bar の Drag observer が伝播しないようにする) ───
    const CLOSE_BTN_SIZE: f32 = 20.0;
    const CLOSE_BTN_MARGIN: f32 = 8.0;
    let close_btn_x = spec.size.x / 2.0 - CLOSE_BTN_SIZE / 2.0 - CLOSE_BTN_MARGIN;
    let close_btn = commands
        .spawn((
            Sprite {
                color: Color::srgba(0.6, 0.15, 0.15, 0.85),
                custom_size: Some(Vec2::splat(CLOSE_BTN_SIZE)),
                ..default()
            },
            Transform::from_xyz(close_btn_x, title_bar_y, 0.2),
            CloseButton,
        ))
        .observe(
            |trigger: Trigger<Pointer<Click>>,
             parent_query: Query<&Parent>,
             root_q: Query<
                (
                    &PanelKind,
                    &Transform,
                    &Sprite,
                    Option<&StrategyEditorId>,
                    Option<&StrategyFragment>,
                    Option<&ChartInstrument>,
                ),
                With<WindowRoot>,
            >,
             mut history: ResMut<AppHistory>,
             mut auto_save: ResMut<crate::ui::layout_persistence::AutoSaveState>,
             mut registry: ResMut<InstrumentRegistry>,
             mut map: ResMut<crate::trading::InstrumentTradingDataMap>,
             mut commands: Commands| {
                let Ok(parent) = parent_query.get(trigger.entity()) else {
                    return;
                };
                let root_entity = parent.get();
                let Ok((kind, tf, sprite, editor_id, fragment, chart_instrument)) =
                    root_q.get(root_entity)
                else {
                    commands.entity(root_entity).despawn_recursive();
                    return;
                };

                // Chart window: editable=false なら何もしない
                if let Some(ci) = chart_instrument {
                    if !registry.editable {
                        return;
                    }
                    registry.remove(&ci.instrument_id);
                    map.map.remove(&ci.instrument_id);
                    commands.entity(root_entity).despawn_recursive();
                    return;
                }

                // 既存ロジック（非 Chart）
                if !history.is_replaying() {
                    let region_key = editor_id.map(|id| id.region_key.clone());
                    let layout = WindowLayout {
                        kind: *kind,
                        region_key,
                        visible: true,
                        position: [tf.translation.x, tf.translation.y],
                        size: sprite
                            .custom_size
                            .map(|s| s.to_array())
                            .unwrap_or([0.0, 0.0]),
                        z: tf.translation.z,
                    };
                    let snapshot = match (editor_id, fragment) {
                        (Some(id), Some(f)) => Some((id.region_key.clone(), f.source.clone())),
                        _ => None,
                    };
                    history.push_window_despawn(layout, snapshot);
                    auto_save.mark_layout_changed(std::time::Instant::now());
                }
                commands.entity(root_entity).despawn_recursive();
            },
        )
        .id();
    commands.entity(root).add_child(close_btn);

    // × テキスト（ボタンの子）
    commands
        .spawn((
            Text2d::new("×"),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Transform::from_xyz(0.0, 0.0, 0.1),
        ))
        .set_parent(close_btn);

    (root, content_area, title_bar)
}

fn fixed_strategy_cache_path() -> Option<std::path::PathBuf> {
    cache_state_paths().map(|(_, cache_py)| cache_py)
}

/// パネル spawn イベントを捌く dispatcher。
/// - 同種の panel が既に world にあれば skip（"一回だけ spawn" ルール）
/// - 無ければ各 PanelKind に対応する spawn 関数を呼ぶ（Sub-step 1.3+ で arm を埋める）
/// - source が User かつ is_replaying でなければ WindowSpawnEdit を AppHistory に push する
pub fn panel_spawn_dispatcher_system(
    mut events: EventReader<PanelSpawnRequested>,
    existing: Query<&PanelKind, With<WindowRoot>>,
    mut commands: Commands,
    mut font_system: ResMut<CosmicFontSystem>,
    mut allocator: ResMut<RegionKeyAllocator>,
    mut history: ResMut<AppHistory>,
    mut pending_fragments: ResMut<PendingStrategyFragments>,
    mut buffer: ResMut<StrategyBuffer>,
) {
    for event in events.read() {
        let already = existing.iter().any(|k| *k == event.kind);
        if already && !matches!(event.kind, PanelKind::StrategyEditor) {
            info!("panel already spawned, skipped: {:?}", event.kind);
            continue;
        }
        let mut spawned_region_key: Option<String> = None;
        match event.kind {
            PanelKind::BuyingPower => spawn_buying_power_panel(&mut commands),
            PanelKind::RunResult => spawn_run_result_panel(&mut commands),
            PanelKind::Positions => spawn_positions_panel(&mut commands),
            PanelKind::Orders => spawn_orders_panel(&mut commands),
            PanelKind::Chart => {
                warn!("PanelKind::Chart spawn requested but Chart is deprecated; ignored");
            }
            PanelKind::StrategyEditor => {
                let spec = event.strategy_spec.clone().unwrap_or_else(|| {
                    let key = allocator.allocate();
                    // strategy がロード済みの場合は既存の cache_path を使う。
                    // original_path は絶対に None に上書きしない（ScenarioMetadata がリセットされる）。
                    if buffer.original_path.is_none() && buffer.cache_path.is_none() {
                        if let Some(temp_path) = fixed_strategy_cache_path() {
                            if let Some(parent) = temp_path.parent() {
                                match std::fs::create_dir_all(parent) {
                                    Ok(()) => {
                                        buffer.cache_path = Some(temp_path);
                                    }
                                    Err(e) => {
                                        warn!(
                                            "fixed strategy cache dir creation failed: {}, Run will be blocked",
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    StrategyEditorSpawnSpec {
                        region_key: Some(key),
                        source: Some(String::new()),
                        layout_source: event.source,
                    }
                });
                let spec = if spec.source.is_none() {
                    let source = spec
                        .region_key
                        .as_deref()
                        .and_then(|k| pending_fragments.by_region_key.remove(k))
                        .unwrap_or_default();
                    StrategyEditorSpawnSpec {
                        source: Some(source),
                        ..spec
                    }
                } else {
                    spec
                };
                spawned_region_key = spec.region_key.clone();
                spawn_strategy_editor_panel(&mut commands, &mut font_system, &mut allocator, spec);
            }
        }
        if event.source == PanelSpawnSource::User && !history.is_replaying() {
            let default_layout = WindowLayout {
                kind: event.kind,
                region_key: spawned_region_key,
                visible: true,
                position: [0.0, 0.0],
                size: [400.0, 300.0],
                z: 10.0,
            };
            history.push_window_spawn(event.kind, default_layout);
        }
    }
}
