use crate::ui::buying_power::spawn_buying_power_panel;
use crate::ui::components::{
    ChartInstrument, ChartSizeMap, CloseButton, InstrumentRegistry, LayoutExcluded, PanelKind,
    PanelSpawnRequested, PanelSpawnSource, PendingStrategyFragments, RegionKeyAllocator,
    StrategyBuffer, StrategyEditorId, StrategyEditorSpawnSpec, StrategyFragment, TitleBar,
    WindowManager, WindowRoot,
};
use crate::ui::editor_history::{ActiveDrag, AppHistory};
use crate::ui::layout_persistence::WindowLayout;
use crate::ui::menu_bar::cache_state_paths;
use crate::ui::orders::spawn_orders_panel;
use crate::ui::positions::spawn_positions_panel;
use crate::ui::run_result_panel::spawn_run_result_panel;
use crate::ui::scenario_startup_panel::spawn_scenario_startup_window;
use crate::ui::order_panel::spawn_order_form_in_window;
use crate::ui::strategy_editor::spawn_strategy_editor_panel;
use bevy::prelude::*;
use bevy_cosmic_edit::prelude::CosmicFontSystem;

/// floating window の title bar 高さ。chart レイアウト定数 (`chart_viewstate.rs`) もこれを参照する
/// (Caveat #33: 二重定義すると chart の draw 領域が枠を ~8px はみ出す)。
pub const TITLE_BAR_HEIGHT: f32 = 40.0;

/// リサイズハンドル sprite の太さ（px）。透明だが bounds picking が有効なので hitbox として機能する。
const RESIZE_HANDLE_THICKNESS: f32 = 8.0;

/// floating window の最小サイズ。ドラッグによるリサイズでこれより小さくならないようクランプする。
pub const MIN_WINDOW_SIZE: Vec2 = Vec2::new(280.0, 180.0);

/// タイトルテキストの左端 padding（root 左端からの距離）。layout_system でも参照する。
const TITLE_PADDING_LEFT: f32 = 16.0;

/// クローズボタンのサイズ（正方形）。layout_system でも参照する。
const CLOSE_BTN_SIZE: f32 = 20.0;

/// クローズボタンの右端 margin。layout_system でも参照する。
const CLOSE_BTN_MARGIN: f32 = 8.0;

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
    /// × クローズボタンを spawn するか。false の panel（Startup 等）は閉じられない
    pub closeable: bool,
    /// 右端・下端・右下コーナーのドラッグでリサイズ可能にするか
    pub resizable: bool,
}

/// root に挿入する子エンティティのカタログ。layout_system がこれを参照してリサイズ追従する。
#[derive(Component)]
pub struct FloatingWindowChildren {
    pub inner_glow: Entity,
    pub rim_light: Entity,
    pub title_bar: Entity,
    pub title_text: Entity,
    pub close_button: Option<Entity>,
    pub resize_right: Option<Entity>,
    pub resize_bottom: Option<Entity>,
    pub resize_corner: Option<Entity>,
}

/// リサイズハンドルの軸方向。ドラッグ observer 内でクロージャキャプチャする。
#[derive(Clone, Copy)]
enum ResizeAxis {
    Right,
    Bottom,
    Corner,
}

/// 透明なリサイズハンドル sprite を spawn し、Drag/DragEnd/Over/Out の 4 observer を付けて返す。
/// caller が root の子として add_child する。
fn spawn_resize_handle(commands: &mut Commands, axis: ResizeAxis, size: Vec2, pos: Vec2) -> Entity {
    commands
        .spawn((
            Sprite {
                color: Color::srgba(0.0, 0.0, 0.0, 0.0),
                custom_size: Some(size),
                ..default()
            },
            Transform::from_xyz(pos.x, pos.y, 0.5),
        ))
        // Drag → root の custom_size / translation を更新（左端・上端固定）
        .observe(
            move |drag: Trigger<Pointer<Drag>>,
                  parent_q: Query<&Parent>,
                  mut root_q: Query<(&mut Sprite, &mut Transform), With<WindowRoot>>,
                  camera_q: Query<&OrthographicProjection, With<Camera2d>>| {
                if drag.event().button != PointerButton::Primary {
                    return;
                }
                let Ok(parent) = parent_q.get(drag.entity()) else {
                    return;
                };
                let scale = camera_q.get_single().map(|p| p.scale).unwrap_or(1.0);
                let dx = drag.event().delta.x * scale;
                let dy = drag.event().delta.y * scale; // screen down (+) = height increase
                let Ok((mut sprite, mut tf)) = root_q.get_mut(parent.get()) else {
                    return;
                };
                let Some(cur) = sprite.custom_size else {
                    return;
                };
                let (dw, dh) = match axis {
                    ResizeAxis::Right => (dx, 0.0),
                    ResizeAxis::Bottom => (0.0, dy),
                    ResizeAxis::Corner => (dx, dy),
                };
                let new_w = (cur.x + dw).max(MIN_WINDOW_SIZE.x);
                let new_h = (cur.y + dh).max(MIN_WINDOW_SIZE.y);
                let adw = new_w - cur.x;
                let adh = new_h - cur.y;
                sprite.custom_size = Some(Vec2::new(new_w, new_h));
                tf.translation.x += adw / 2.0;
                tf.translation.y -= adh / 2.0; // world y は screen y の逆
            },
        )
        // DragEnd → autosave をマーク（chart は ChartSizeMap にサイズを保存）
        .observe(
            |end: Trigger<Pointer<DragEnd>>,
             parent_q: Query<&Parent>,
             root_q: Query<(Option<&ChartInstrument>, Option<&Sprite>), With<WindowRoot>>,
             mut auto_save: ResMut<crate::ui::layout_persistence::AutoSaveState>,
             mut chart_sizes: ResMut<ChartSizeMap>| {
                let Ok(parent) = parent_q.get(end.entity()) else {
                    return;
                };
                let Ok((chart_opt, sprite_opt)) = root_q.get(parent.get()) else {
                    return;
                };
                if let Some(chart_instrument) = chart_opt {
                    // chart resize → instrument_id をキーにサイズを保存
                    if let Some(size) = sprite_opt.and_then(|s| s.custom_size) {
                        chart_sizes
                            .map
                            .insert(chart_instrument.instrument_id.clone(), size);
                    }
                    return;
                }
                auto_save.mark_layout_changed(std::time::Instant::now());
            },
        )
        // Over → リサイズカーソルに変更（Window entity に CursorIcon component を insert）
        .observe(
            move |_: Trigger<Pointer<Over>>,
                  mut commands: Commands,
                  windows: Query<Entity, With<bevy::window::PrimaryWindow>>| {
                use bevy::window::SystemCursorIcon;
                use bevy::winit::cursor::CursorIcon;
                if let Ok(entity) = windows.get_single() {
                    let icon = match axis {
                        ResizeAxis::Right => SystemCursorIcon::EwResize,
                        ResizeAxis::Bottom => SystemCursorIcon::NsResize,
                        ResizeAxis::Corner => SystemCursorIcon::SeResize,
                    };
                    commands.entity(entity).insert(CursorIcon::from(icon));
                }
            },
        )
        // Out → デフォルトカーソルに戻す
        .observe(
            |_: Trigger<Pointer<Out>>,
             mut commands: Commands,
             windows: Query<Entity, With<bevy::window::PrimaryWindow>>| {
                use bevy::window::SystemCursorIcon;
                use bevy::winit::cursor::CursorIcon;
                if let Ok(entity) = windows.get_single() {
                    commands
                        .entity(entity)
                        .insert(CursorIcon::from(SystemCursorIcon::Default));
                }
            },
        )
        .id()
}

/// 戻り値: (root_entity, content_area_entity, title_bar_entity)
/// - root_entity: ウィンドウ全体の親。位置を動かしたいときはこれを動かす
/// - content_area_entity: タイトルバーの下の領域。中身（チャート・テキストなど）はここの子にする
/// - title_bar_entity: タイトルバー sprite。タイトル右端にボタンを足したい panel 用に公開する
pub fn spawn_floating_window(
    commands: &mut Commands,
    spec: FloatingWindowSpec,
) -> (Entity, Entity, Entity) {
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
    let inner_glow = commands
        .spawn((
            Sprite {
                color: Color::srgba(1.0, 1.0, 1.0, 0.05),
                custom_size: Some(spec.size - Vec2::splat(4.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.01),
        ))
        .id();
    commands.entity(root).add_child(inner_glow);

    // ─── 3. Rim light (外周の色付き発光、accent 色を使う) ───
    let rim_light = commands
        .spawn((
            Sprite {
                color: spec.accent,
                // rim は両側に 1px ずつはみ出す → 幅・高さ +2
                custom_size: Some(spec.size + Vec2::splat(2.0)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, -0.01),
        ))
        .id();
    commands.entity(root).add_child(rim_light);

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
    // `Visibility` を明示付与する（→ required components で `InheritedVisibility` も付く）。
    // これが無いと content_area は可視性伝播の連鎖を断ち切り、root を `Visibility::Hidden`
    // にしても子（ラベル/フィールド）が隠れない（`propagate_recursive` が
    // `(&Visibility, &mut InheritedVisibility)` の get に失敗して early-return するため）。
    // Startup パネルを Manual/Auto で隠すケースで顕在化した（root 枠は消えるが中身が残る）。
    let content_area = commands
        .spawn((
            Transform::from_xyz(0.0, -title_bar_half, 0.1),
            Visibility::default(),
        ))
        .id();
    commands.entity(root).add_child(content_area);

    // ─── 7. Close button (× — タイトルバー右端。root 直下に置くことで
    //        title_bar の Drag observer が伝播しないようにする) ───
    let mut close_button_entity: Option<Entity> = None;
    if spec.closeable {
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
                            (Some(id), Some(f)) => {
                                Some((id.region_key.clone(), f.source.clone()))
                            }
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

        close_button_entity = Some(close_btn);
    }

    // ─── 8. Resize handles (resizable が true のときのみ) ───
    let (resize_right, resize_bottom, resize_corner) = if spec.resizable {
        let w = spec.size.x;
        let h = spec.size.y;

        let right = spawn_resize_handle(
            commands,
            ResizeAxis::Right,
            Vec2::new(RESIZE_HANDLE_THICKNESS, h),
            Vec2::new(w / 2.0, 0.0),
        );
        commands.entity(root).add_child(right);

        let bottom = spawn_resize_handle(
            commands,
            ResizeAxis::Bottom,
            Vec2::new(w, RESIZE_HANDLE_THICKNESS),
            Vec2::new(0.0, -h / 2.0),
        );
        commands.entity(root).add_child(bottom);

        let corner = spawn_resize_handle(
            commands,
            ResizeAxis::Corner,
            Vec2::new(RESIZE_HANDLE_THICKNESS, RESIZE_HANDLE_THICKNESS),
            Vec2::new(w / 2.0, -h / 2.0),
        );
        commands.entity(root).add_child(corner);

        (Some(right), Some(bottom), Some(corner))
    } else {
        (None, None, None)
    };

    // ─── 9. FloatingWindowChildren を root に挿入 ───
    commands.entity(root).insert(FloatingWindowChildren {
        inner_glow,
        rim_light,
        title_bar,
        title_text,
        close_button: close_button_entity,
        resize_right,
        resize_bottom,
        resize_corner,
    });

    (root, content_area, title_bar)
}

/// root の custom_size が変わったとき、FloatingWindowChildren が保持する子エンティティを
/// 新しいサイズに追従させる。差分書き込み（規約 2）で change detection の無駄発火を防ぐ。
pub fn floating_window_layout_system(
    roots: Query<(&Sprite, &FloatingWindowChildren), (With<WindowRoot>, Changed<Sprite>)>,
    mut sprites: Query<&mut Sprite, Without<WindowRoot>>,
    mut transforms: Query<&mut Transform, Without<WindowRoot>>,
) {
    for (root_sprite, children) in &roots {
        let Some(size) = root_sprite.custom_size else {
            continue;
        };
        let w = size.x;
        let h = size.y;
        let title_bar_half = TITLE_BAR_HEIGHT / 2.0;
        let title_bar_y = h / 2.0 - title_bar_half;

        // inner_glow
        if let Ok(mut s) = sprites.get_mut(children.inner_glow) {
            let target = size - Vec2::splat(4.0);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }

        // rim_light
        if let Ok(mut s) = sprites.get_mut(children.rim_light) {
            let target = size + Vec2::splat(2.0);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }

        // title_bar: 幅を更新、y 位置も更新（高さが変わるとタイトルバーが上端に来るべき）
        if let Ok(mut s) = sprites.get_mut(children.title_bar) {
            let target = Vec2::new(w, TITLE_BAR_HEIGHT);
            if s.custom_size != Some(target) {
                s.custom_size = Some(target);
            }
        }
        if let Ok(mut t) = transforms.get_mut(children.title_bar) {
            if (t.translation.y - title_bar_y).abs() > 0.01 {
                t.translation.y = title_bar_y;
            }
        }

        // title_text: x 位置を更新
        if let Ok(mut t) = transforms.get_mut(children.title_text) {
            let target_x = -w / 2.0 + TITLE_PADDING_LEFT;
            if (t.translation.x - target_x).abs() > 0.01 {
                t.translation.x = target_x;
            }
        }

        // close_button: x / y 位置を更新
        if let Some(cb) = children.close_button {
            if let Ok(mut t) = transforms.get_mut(cb) {
                let target_x = w / 2.0 - CLOSE_BTN_SIZE / 2.0 - CLOSE_BTN_MARGIN;
                if (t.translation.x - target_x).abs() > 0.01 {
                    t.translation.x = target_x;
                }
                if (t.translation.y - title_bar_y).abs() > 0.01 {
                    t.translation.y = title_bar_y;
                }
            }
        }

        // resize_right ハンドル: 高さと x 位置
        if let Some(e) = children.resize_right {
            if let Ok(mut s) = sprites.get_mut(e) {
                let target = Vec2::new(RESIZE_HANDLE_THICKNESS, h);
                if s.custom_size != Some(target) {
                    s.custom_size = Some(target);
                }
            }
            if let Ok(mut t) = transforms.get_mut(e) {
                let target_x = w / 2.0;
                if (t.translation.x - target_x).abs() > 0.01 {
                    t.translation.x = target_x;
                }
            }
        }

        // resize_bottom ハンドル: 幅と y 位置
        if let Some(e) = children.resize_bottom {
            if let Ok(mut s) = sprites.get_mut(e) {
                let target = Vec2::new(w, RESIZE_HANDLE_THICKNESS);
                if s.custom_size != Some(target) {
                    s.custom_size = Some(target);
                }
            }
            if let Ok(mut t) = transforms.get_mut(e) {
                let target_y = -h / 2.0;
                if (t.translation.y - target_y).abs() > 0.01 {
                    t.translation.y = target_y;
                }
            }
        }

        // resize_corner ハンドル: x / y 位置
        if let Some(e) = children.resize_corner {
            if let Ok(mut t) = transforms.get_mut(e) {
                let target_x = w / 2.0;
                let target_y = -h / 2.0;
                if (t.translation.x - target_x).abs() > 0.01 {
                    t.translation.x = target_x;
                }
                if (t.translation.y - target_y).abs() > 0.01 {
                    t.translation.y = target_y;
                }
            }
        }
    }
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
            PanelKind::Startup => spawn_scenario_startup_window(&mut commands),
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
            PanelKind::Order => {
                let (root, content_area, _title_bar) = spawn_floating_window(
                    &mut commands,
                    FloatingWindowSpec {
                        title: "ORDER".to_string(),
                        size: Vec2::new(320.0, 360.0),
                        position: Vec2::new(0.0, 0.0),
                        accent: Color::srgb(0.20, 0.80, 1.0),
                        closeable: true,
                        resizable: false,
                    },
                );
                commands.entity(root).insert((PanelKind::Order, LayoutExcluded));
                spawn_order_form_in_window(&mut commands, content_area);
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

#[cfg(test)]
mod close_button_tests {
    use super::*;
    use crate::ui::components::{CloseButton, WindowManager};
    use crate::ui::editor_history::{ActiveDrag, AppHistory};

    fn spawn_test_window(closeable: bool) -> (App, Entity) {
        let mut app = App::new();
        app.init_resource::<WindowManager>();
        app.init_resource::<ActiveDrag>();
        app.init_resource::<AppHistory>();
        let root = {
            let world = app.world_mut();
            let mut commands_queue = bevy::ecs::world::CommandQueue::default();
            let mut commands = Commands::new(&mut commands_queue, world);
            let (root, _content, _title) = spawn_floating_window(
                &mut commands,
                FloatingWindowSpec {
                    title: "T".to_string(),
                    size: Vec2::new(100.0, 100.0),
                    position: Vec2::ZERO,
                    accent: Color::WHITE,
                    closeable,
                    resizable: false,
                },
            );
            commands_queue.apply(world);
            root
        };
        (app, root)
    }

    #[test]
    fn closeable_true_spawns_close_button() {
        let (app, _root) = spawn_test_window(true);
        let count = app
            .world()
            .iter_entities()
            .filter(|e| e.contains::<CloseButton>())
            .count();
        assert_eq!(count, 1, "closeable:true は × ボタンを 1 個 spawn する");
    }

    #[test]
    fn closeable_false_spawns_no_close_button() {
        let (app, _root) = spawn_test_window(false);
        let count = app
            .world()
            .iter_entities()
            .filter(|e| e.contains::<CloseButton>())
            .count();
        assert_eq!(count, 0, "closeable:false は × ボタンを spawn しない");
    }
}

#[cfg(test)]
mod order_dispatcher_tests {
    use super::*;
    use crate::ui::components::{
        PanelSpawnRequested, PanelSpawnSource, PendingStrategyFragments, RegionKeyAllocator,
        StrategyBuffer,
    };
    use crate::ui::editor_history::AppHistory;
    use bevy_cosmic_edit::cosmic_text::FontSystem;
    use bevy_cosmic_edit::prelude::CosmicFontSystem;

    fn order_dispatch_app() -> App {
        let mut app = App::new();
        app.insert_resource(CosmicFontSystem(FontSystem::new()));
        app.init_resource::<WindowManager>();
        app.init_resource::<ActiveDrag>();
        app.init_resource::<RegionKeyAllocator>();
        app.init_resource::<AppHistory>();
        app.init_resource::<PendingStrategyFragments>();
        app.init_resource::<StrategyBuffer>();
        app.add_event::<PanelSpawnRequested>();
        app.add_systems(Update, panel_spawn_dispatcher_system);
        app
    }

    fn order_panel_count(app: &App) -> usize {
        app.world()
            .iter_entities()
            .filter(|e| {
                e.contains::<WindowRoot>()
                    && e.get::<PanelKind>() == Some(&PanelKind::Order)
            })
            .count()
    }

    #[test]
    fn order_request_spawns_exactly_one_window() {
        let mut app = order_dispatch_app();
        app.world_mut().send_event(PanelSpawnRequested {
            kind: PanelKind::Order,
            source: PanelSpawnSource::User,
            strategy_spec: None,
        });
        app.update();
        assert_eq!(order_panel_count(&app), 1, "Order request spawns exactly 1 window");
    }

    #[test]
    fn duplicate_order_request_does_not_spawn_second_window() {
        let mut app = order_dispatch_app();
        for _ in 0..2 {
            app.world_mut().send_event(PanelSpawnRequested {
                kind: PanelKind::Order,
                source: PanelSpawnSource::User,
                strategy_spec: None,
            });
            app.update();
        }
        assert_eq!(order_panel_count(&app), 1, "dedup guard holds: still exactly 1 Order window");
    }
}
