use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_pancam::{DirectionKeys, PanCam};

use crate::ui::chart_viewstate::ChartViewState;
use crate::ui::order_context_menu::OrderContextMenu;
use crate::ui::strategy_editor::StrategyEditorRoot;

pub fn setup_camera(mut commands: Commands) {
    commands.spawn((
        Camera2d,
        PanCam {
            grab_buttons: vec![MouseButton::Right, MouseButton::Middle],
            move_keys: DirectionKeys::NONE, // disable AWSD pan — conflicts with bevscode editor typing
            ..default()
        },
    ));
}

/// AABB ヒット判定の純粋関数。`cursor_world` が None なら常に false。
/// system からも test からも呼べるようにここに切り出す（headless test が
/// `viewport_to_world_2d` を経由せず判定できる）。
pub(crate) fn cursor_hits_any_sprite<'a>(
    cursor_world: Option<Vec2>,
    sprites: impl IntoIterator<Item = (&'a GlobalTransform, &'a Sprite)>,
) -> bool {
    let Some(cursor) = cursor_world else {
        return false;
    };
    sprites.into_iter().any(|(transform, sprite)| {
        let size = sprite.custom_size.unwrap_or(Vec2::ONE);
        let center = transform.translation().truncate();
        let half = size / 2.0;
        cursor.x > center.x - half.x
            && cursor.x < center.x + half.x
            && cursor.y > center.y - half.y
            && cursor.y < center.y + half.y
    })
}

/// マウスホイールの二重ハンドリング（PanCam のカメラズーム vs bevscode のエディタスクロール）を
/// フレーム単位で排他制御する。
///
/// - カーソルが Strategy Editor のスプライト矩形上にあり、かつ Ctrl 非押下:
///   PanCam を無効化（カメラがズーム/パンしない）し、bevscode 側にホイールを譲る。
/// - それ以外（Ctrl 押下中、またはカーソルがエディタ外）:
///   PanCam を有効化する。
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
    editor_q: Query<(&GlobalTransform, &Sprite), With<StrategyEditorRoot>>,
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

    let over_editor = cursor_hits_any_sprite(cursor_world, editor_q.iter());
    let over_chart = cursor_hits_any_sprite(cursor_world, chart_q.iter());

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
}

#[cfg(test)]
mod tests {
    //! Slice 6b (#50): `cursor_hits_any_sprite` の AABB 判定が
    //! sprite 矩形の内外/None を正しく分けることを assert する pure fn test。
    //!
    //! system 全体（PanCam 切替、Ctrl 押下、dragging、context_menu）の挙動は
    //! `viewport_to_world_2d` 等の Bevy ランタイム経路に依存するためここでは扱わず、
    //! 必要なら e2e_replay に格上げする。

    use super::*;

    /// 500x400 sprite を world origin に置いた fixture を返す。
    /// borrow を維持するため (GlobalTransform, Sprite) を呼び出し側でローカルに束縛し、
    /// `[(&gt, &sprite)]` を `cursor_hits_any_sprite` に渡す形を取る。
    fn fixture_root_500x400() -> (GlobalTransform, Sprite) {
        let sprite = Sprite {
            custom_size: Some(Vec2::new(500.0, 400.0)),
            ..default()
        };
        let gt = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0));
        (gt, sprite)
    }

    #[test]
    fn cursor_inside_sprite_hits() {
        let (gt, sprite) = fixture_root_500x400();
        assert!(cursor_hits_any_sprite(
            Some(Vec2::ZERO),
            [(&gt, &sprite)],
        ));
    }

    #[test]
    fn cursor_outside_sprite_misses() {
        let (gt, sprite) = fixture_root_500x400();
        assert!(!cursor_hits_any_sprite(
            Some(Vec2::new(1000.0, 0.0)),
            [(&gt, &sprite)],
        ));
    }

    #[test]
    fn cursor_none_returns_false() {
        let (gt, sprite) = fixture_root_500x400();
        assert!(!cursor_hits_any_sprite(
            None,
            [(&gt, &sprite)],
        ));
    }
}
