//! 透明ヒット領域 Sprite helper。
//!
//! World-space Camera2d シーン内に `Sprite + Transform + Pickable` として
//! 配置する hit 領域を spawn する。`Node / ZIndex` とは無関係。
//!
//! ## なぜ alpha = 0.001 か
//! `bevy_picking` の `SpritePickingPlugin` は alpha == 0.0 の Sprite を
//! AlphaThreshold でピッキング対象外にする。0.001 にすることで視覚的には
//! 不可視のまま picking を有効にする。

use bevy::picking::Pickable;
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Public helper
// ---------------------------------------------------------------------------

/// 透明な矩形ヒット領域を world 空間に spawn する。
///
/// - `size`  : ヒット矩形の幅・高さ（ピクセル）
/// - `pos`   : `Transform.translation`（z で描画順を調整する）
///
/// 返り値は spawn した entity の id。
/// 呼び出し元は `.observe(...)` などでイベントを登録すること。
pub fn spawn_transparent_hit_sprite(
    commands: &mut Commands,
    size: Vec2,
    pos: Vec3,
) -> Entity {
    commands
        .spawn((
            Sprite {
                color: Color::WHITE.with_alpha(0.001),
                custom_size: Some(size),
                ..default()
            },
            Transform::from_translation(pos),
            Pickable::default(),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn transparent_hit_sprite_has_correct_components() {
        let mut world = World::new();

        let entity = world
            .run_system_once(|mut commands: Commands| {
                spawn_transparent_hit_sprite(
                    &mut commands,
                    Vec2::new(100.0, 20.0),
                    Vec3::new(1.0, 2.0, 0.05),
                )
            })
            .unwrap();

        // Sprite.custom_size
        let sprite = world.get::<Sprite>(entity).expect("Sprite missing");
        assert_eq!(
            sprite.custom_size,
            Some(Vec2::new(100.0, 20.0)),
            "custom_size mismatch"
        );

        // alpha > 0.0 かつ < 0.01（picking 有効・視覚的不可視）
        let alpha = sprite.color.alpha();
        assert!(alpha > 0.0, "alpha must be > 0.0 (picking exclusion guard)");
        assert!(alpha < 0.01, "alpha must be < 0.01 (visually invisible)");

        // Pickable が存在する
        assert!(
            world.get::<Pickable>(entity).is_some(),
            "Pickable component missing"
        );

        // Transform.translation
        let tf = world.get::<Transform>(entity).expect("Transform missing");
        assert_eq!(
            tf.translation,
            Vec3::new(1.0, 2.0, 0.05),
            "translation mismatch"
        );
    }
}
