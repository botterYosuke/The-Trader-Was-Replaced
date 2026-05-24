use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_pancam::{DirectionKeys, PanCam};

use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::order_context_menu::OrderContextMenu;

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            move_keys: DirectionKeys::NONE, // disable AWSD pan
            ..default()
        },
    ));
}

/// マウスホイールの二重ハンドリング（PanCam のカメラズーム vs chart のスクロールズーム）を
/// フレーム単位で排他制御する。
///
/// - カーソルが chart のドロー領域（`ChartViewState` を持つ Sprite）上にあり、かつ Ctrl 非押下:
///   PanCam を無効化（カメラがズーム/パンしない）。これにより chart 上のホイールが
///   `chart_scroll_zoom_system` の chart ズームのみに効き、カメラズームと二重発火しない
///   (bevy-engine スキル「ホイール二重消費」罠)。
/// - それ以外（Ctrl 押下中、またはカーソルが chart 外）: PanCam を有効化。
///
/// Ctrl 押下中は「キャンバス全体をズームしたい」意図とみなし、chart 上でも PanCam を優先する。
///
/// Strategy Editor は screen-space Bevy UI（`bevy_ui_text_input`）になり world-space sprite
/// ではなくなったため、editor 上のホイール抑制（旧 `ScrollEnabled` / `CosmicPrimaryCamera` 経路）は
/// 撤去した（editor は UI ノードとして自前で入力/スクロールを処理する）。ADR 0003 参照。
///
/// この system は `main.rs` で `PanCamSystemSet` より前に走らせる必要がある。
/// 後ろに置くと、PanCam の do_camera_zoom が `enabled` を読んだ後に書き換えることになり 1 フレーム遅れる。
pub fn pancam_suppression_over_editor_system(
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut camera_q: Query<(&mut PanCam, &Camera, &GlobalTransform)>,
    chart_q: Query<(&GlobalTransform, &Sprite), With<ChartViewState>>,
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

    // カーソルがいずれかの chart ドロー領域の内側にあるか。
    let over_chart = match cursor_world {
        Some(cursor) => chart_q
            .iter()
            .any(|(transform, sprite)| cursor_in(cursor, transform, sprite)),
        None => false,
    };

    // 右/中ボタンドラッグ中は chart 上でも強制的にパンを有効にする。
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    // OrdersPanel 右クリックコンテキストメニューが開いている間は PanCam を止める。
    // PanCam は Right ボタンを掴む (grab_buttons) ため、メニューを開く右クリック自体が
    // pan を誘発し、screen-space のメニューと world-space の OrdersPanel がずれる。
    let context_menu_open = context_menu.map(|m| m.open).unwrap_or(false);
    // 値が変わるときだけ書く: 毎フレームの無条件代入は spurious な Changed<PanCam> を立てる。
    let should_enable = !context_menu_open && (ctrl || !over_chart || dragging);
    if pancam.enabled != should_enable {
        pancam.enabled = should_enable;
    }
}
