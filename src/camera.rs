use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_cosmic_edit::ScrollEnabled;
use bevy_cosmic_edit::prelude::CosmicPrimaryCamera;
use bevy_pancam::{DirectionKeys, PanCam};

use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::order_context_menu::OrderContextMenu;
use crate::ui::strategy_editor::StrategyEditorContent;

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        CosmicPrimaryCamera,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            move_keys: DirectionKeys::NONE, // disable AWSD pan — conflicts with cosmic_edit
            ..default()
        },
    ));
}

/// マウスホイールの二重ハンドリング（PanCam のカメラズーム vs cosmic_edit のエディタスクロール）を
/// フレーム単位で排他制御する。
///
/// - カーソルが Strategy Editor のスプライト矩形上にあり、かつ Ctrl 非押下:
///   PanCam を無効化（カメラがズーム/パンしない）し、エディタのスクロールを有効化。
/// - それ以外（Ctrl 押下中、またはカーソルがエディタ外）:
///   PanCam を有効化し、エディタのスクロールを無効化。
///
/// Phase C (chart pan/zoom): chart のドロー領域 (`ChartViewState` を持つ Sprite) 上でも
/// Ctrl 非押下なら PanCam を無効化する。これにより chart 上のホイールが
/// `chart_scroll_zoom_system` の chart ズームのみに効き、カメラズームと二重発火しない
/// (bevy-engine スキル「ホイール二重消費」罠)。chart 側 zoom も Ctrl 押下時は skip するので
/// 「Ctrl+ホイール = キャンバス全体ズーム / ホイール = chart ズーム」で対称になる。
///
/// Ctrl 押下中は「キャンバス全体をズームしたい」意図とみなし、エディタ/chart 上でも PanCam を優先する。
///
/// この system は `main.rs` で `PanCamSystemSet` より前に走らせる必要がある。
/// 後ろに置くと、PanCam の do_camera_zoom が `enabled` を読んだ後に書き換えることになり 1 フレーム遅れる。
pub fn pancam_suppression_over_editor_system(
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut camera_q: Query<(&mut PanCam, &Camera, &GlobalTransform)>,
    editor_q: Query<(&GlobalTransform, &Sprite), With<StrategyEditorContent>>,
    chart_q: Query<(&GlobalTransform, &Sprite), With<ChartViewState>>,
    mut scroll_q: Query<&mut ScrollEnabled, With<StrategyEditorContent>>,
    context_menu: Option<Res<OrderContextMenu>>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((mut pancam, camera, camera_transform)) = camera_q.single_mut() else {
        return;
    };

    // カーソルのスクリーン座標 → ワールド座標。ウィンドウ外 / 変換失敗時は None。
    let cursor_world = window.cursor_position().and_then(|screen_pos| {
        camera
            .viewport_to_world_2d(camera_transform, screen_pos)
            .ok()
    });

    // カーソルがあるスプライト矩形の内側にあるか（AABB 判定）。
    let cursor_in = |cursor: Vec2, transform: &GlobalTransform, sprite: &Sprite| {
        // custom_size 未設定のスプライトは 1x1 として実質ヒットしない扱い。
        let size = sprite.custom_size.unwrap_or(Vec2::ONE);
        let center = transform.translation().truncate();
        let half = size / 2.0;
        cursor.x > center.x - half.x
            && cursor.x < center.x + half.x
            && cursor.y > center.y - half.y
            && cursor.y < center.y + half.y
    };

    // カーソルがいずれかの Strategy Editor スプライト矩形の内側にあるか。
    let over_editor = match cursor_world {
        Some(cursor) => editor_q
            .iter()
            .any(|(transform, sprite)| cursor_in(cursor, transform, sprite)),
        None => false,
    };
    // カーソルがいずれかの chart ドロー領域 (Phase C) の内側にあるか。
    let over_chart = match cursor_world {
        Some(cursor) => chart_q
            .iter()
            .any(|(transform, sprite)| cursor_in(cursor, transform, sprite)),
        None => false,
    };

    // 右/中ボタンドラッグ中はエディタ/chart 上でも強制的にパンを有効にする。
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    // OrdersPanel 右クリックコンテキストメニューが開いている間は PanCam を止める。
    // PanCam は Right ボタンを掴む (grab_buttons) ため、メニューを開く右クリック自体が
    // pan を誘発し、screen-space のメニューと world-space の OrdersPanel がずれる。
    // メニュー表示中は `dragging` を上書きして強制的に無効化する。
    let context_menu_open = context_menu.map(|m| m.open).unwrap_or(false);
    // 値が変わるときだけ書く: 毎フレームの無条件代入は spurious な Changed<PanCam> を立てる。
    let should_enable = !context_menu_open && (ctrl || !(over_editor || over_chart) || dragging);
    if pancam.enabled != should_enable {
        pancam.enabled = should_enable;
    }

    // エディタスクロールが効くのは「エディタ上 かつ Ctrl 非押下」のフレームだけ。
    let editor_should_scroll = over_editor && !ctrl;
    for mut scroll_enabled in &mut scroll_q {
        *scroll_enabled = if editor_should_scroll {
            ScrollEnabled::Enabled
        } else {
            ScrollEnabled::Disabled
        };
    }
}
