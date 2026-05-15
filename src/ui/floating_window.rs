use crate::ui::buying_power::spawn_buying_power_panel;
use crate::ui::components::{CloseButton, PanelKind, PanelSpawnRequested, PanelSpawnSource, StrategyBuffer, TitleBar, WindowManager, WindowRoot};
use crate::ui::editor_history::{ActiveDrag, AppHistory};
use crate::ui::layout_persistence::WindowLayout;
use crate::ui::orders::spawn_orders_panel;
use crate::ui::positions::spawn_positions_panel;
use crate::ui::run_result_panel::spawn_run_result_panel;
use crate::ui::strategy_editor::spawn_strategy_editor_panel;
use crate::ui::window::spawn_chart_panel;
use bevy::prelude::*;
use bevy_cosmic_edit::prelude::CosmicFontSystem;

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
    const TITLE_BAR_HEIGHT: f32 = 40.0;
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
                if let Ok(parent) = parent_query.get(drag.entity()) {
                    if let Ok(mut transform) = query.get_mut(parent.get()) {
                        let scale = camera_query.get_single().map(|p| p.scale).unwrap_or(1.0);
                        transform.translation.x += drag.event().delta.x * scale;
                        transform.translation.y -= drag.event().delta.y * scale;
                    }
                }
            },
        )
        .observe(
            |drag_start: Trigger<Pointer<DragStart>>,
             parent_query: Query<&Parent>,
             root_q: Query<&Transform, With<WindowRoot>>,
             mut active_drag: ResMut<ActiveDrag>| {
                if let Ok(parent) = parent_query.get(drag_start.entity()) {
                    let root_entity = parent.get();
                    if let Ok(tf) = root_q.get(root_entity) {
                        active_drag.starts.insert(root_entity, tf.translation.truncate());
                    }
                }
            },
        )
        .observe(
            |drag_end: Trigger<Pointer<DragEnd>>,
             parent_query: Query<&Parent>,
             root_q: Query<(&Transform, &PanelKind), With<WindowRoot>>,
             mut active_drag: ResMut<ActiveDrag>,
             mut history: ResMut<AppHistory>| {
                if let Ok(parent) = parent_query.get(drag_end.entity()) {
                    let root_entity = parent.get();
                    if let Some(before) = active_drag.starts.remove(&root_entity) {
                        if let Ok((tf, kind)) = root_q.get(root_entity) {
                            let after = tf.translation.truncate();
                            history.push_window_move(*kind, before, after);
                        }
                    }
                }
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
             root_q: Query<(&PanelKind, &Transform, &Sprite), With<WindowRoot>>,
             buffer: Res<StrategyBuffer>,
             mut history: ResMut<AppHistory>,
             mut commands: Commands| {
                // CloseButton の親が root なので 1 hop で辿れる
                if let Ok(parent) = parent_query.get(trigger.entity()) {
                    let root_entity = parent.get();
                    // undo 履歴に despawn を記録（閉じた瞬間の位置・サイズ・z をスナップショット）
                    if !history.is_replaying() {
                        if let Ok((kind, tf, sprite)) = root_q.get(root_entity) {
                            let layout = WindowLayout {
                                kind: *kind,
                                visible: true,
                                position: [tf.translation.x, tf.translation.y],
                                size: sprite.custom_size.map(|s| s.to_array()).unwrap_or([0.0, 0.0]),
                                z: tf.translation.z,
                            };
                            let snapshot = if *kind == PanelKind::StrategyEditor {
                                Some(buffer.source.clone())
                            } else {
                                None
                            };
                            history.push_window_despawn(layout, snapshot);
                        }
                    }
                    commands.entity(root_entity).despawn_recursive();
                }
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

/// パネル spawn イベントを捌く dispatcher。
/// - 同種の panel が既に world にあれば skip（"一回だけ spawn" ルール）
/// - 無ければ各 PanelKind に対応する spawn 関数を呼ぶ（Sub-step 1.3+ で arm を埋める）
/// - source が User かつ is_replaying でなければ WindowSpawnEdit を AppHistory に push する
pub fn panel_spawn_dispatcher_system(
    mut events: EventReader<PanelSpawnRequested>,
    existing: Query<&PanelKind, With<WindowRoot>>,
    mut commands: Commands,
    mut font_system: ResMut<CosmicFontSystem>,
    mut history: ResMut<AppHistory>,
) {
    for event in events.read() {
        let already = existing.iter().any(|k| *k == event.kind);
        if already {
            info!("panel already spawned, skipped: {:?}", event.kind);
            continue;
        }
        match event.kind {
            PanelKind::BuyingPower => spawn_buying_power_panel(&mut commands),
            PanelKind::RunResult => spawn_run_result_panel(&mut commands),
            PanelKind::Positions => spawn_positions_panel(&mut commands),
            PanelKind::Orders => spawn_orders_panel(&mut commands),
            PanelKind::Chart => spawn_chart_panel(&mut commands),
            PanelKind::StrategyEditor => {
                spawn_strategy_editor_panel(&mut commands, &mut font_system)
            }
        }
        // User 操作による spawn のみ Undo スタックに記録する
        // dispatcher は spawn 直後に entity の位置を読めない（commands は deferred）ため、
        // デフォルト位置を仮値として渡す（redo 時はデフォルト位置に再 spawn される仕様として許容）。
        if event.source == PanelSpawnSource::User && !history.is_replaying() {
            let default_layout = WindowLayout {
                kind: event.kind,
                visible: true,
                position: [0.0, 0.0],     // deferred spawn のため実位置は不明
                size: [400.0, 300.0],     // デフォルトサイズの仮値
                z: 10.0,
            };
            history.push_window_spawn(event.kind, default_layout);
        }
    }
}
