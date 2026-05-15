use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_cosmic_edit::ScrollEnabled;
use bevy_cosmic_edit::prelude::CosmicPrimaryCamera;
use bevy_pancam::{DirectionKeys, PanCam};

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
/// Ctrl 押下中は「キャンバス全体をズームしたい」意図とみなし、エディタ上でも PanCam を優先する。
///
/// この system は `main.rs` で `PanCamSystemSet` より前に走らせる必要がある。
/// 後ろに置くと、PanCam の do_camera_zoom が `enabled` を読んだ後に書き換えることになり 1 フレーム遅れる。
pub fn pancam_suppression_over_editor_system(
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut camera_q: Query<(&mut PanCam, &Camera, &GlobalTransform)>,
    editor_q: Query<(&GlobalTransform, &Sprite), With<StrategyEditorContent>>,
    mut scroll_q: Query<&mut ScrollEnabled, With<StrategyEditorContent>>,
) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);

    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok((mut pancam, camera, camera_transform)) = camera_q.get_single_mut() else {
        return;
    };

    // カーソルのスクリーン座標 → ワールド座標。ウィンドウ外 / 変換失敗時は None。
    let cursor_world = window.cursor_position().and_then(|screen_pos| {
        camera
            .viewport_to_world_2d(camera_transform, screen_pos)
            .ok()
    });

    // カーソルがいずれかの Strategy Editor スプライト矩形の内側にあるか（AABB 判定）。
    let over_editor = match cursor_world {
        Some(cursor) => editor_q.iter().any(|(transform, sprite)| {
            // custom_size 未設定のエディタは存在しない想定だが、その場合は 1x1 として実質ヒットしない扱い。
            let size = sprite.custom_size.unwrap_or(Vec2::ONE);
            let center = transform.translation().truncate();
            let half = size / 2.0;
            cursor.x > center.x - half.x
                && cursor.x < center.x + half.x
                && cursor.y > center.y - half.y
                && cursor.y < center.y + half.y
        }),
        None => false,
    };

    // 右/中ボタンドラッグ中はエディタ上でも強制的にパンを有効にする。
    let dragging = mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle);
    pancam.enabled = ctrl || !over_editor || dragging;

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
