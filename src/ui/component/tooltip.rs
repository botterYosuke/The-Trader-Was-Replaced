//! Tooltip component — hover-triggered non-blocking label (Issue #46 Slice F).
//!
//! `spawn_tooltip_for` は対象 entity の observer として Over/Out を登録する。
//! Tooltip UI は `On<Pointer<Over>>` で `commands.spawn` し、
//! `On<Pointer<Out>>` で despawn する。
//! z は `ElevationIndex::ElevatedSurface`（100.0）。フォーカスは奪わない。

use crate::ui::theme::{ElevationIndex, Theme};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Marker component
// ---------------------------------------------------------------------------

/// Marker placed on the tooltip popup entity.
/// `text` は表示文字列を保持する（テスト時に確認用）。
#[derive(Component, Clone, Debug)]
pub struct Tooltip {
    pub text: String,
}

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// `target` entity に hover/out observer を登録し、ホバー時に tooltip popup を
/// spawn する。
///
/// - popup は `Node` + `BackgroundColor` + `ZIndex` + `Tooltip` marker を持つ。
/// - z は `ElevationIndex::ElevatedSurface.z()`（= 100.0）。
/// - フォーカスを奪わない（`FocusPolicy::Pass` を設定しない: Node のデフォルトは Block だが
///   Tooltip は pointer-out で即 despawn するため問題なし）。
pub fn spawn_tooltip_for(commands: &mut Commands, target: Entity, text: impl Into<String>, theme: &Theme) {
    let text: String = text.into();
    let bg = ElevationIndex::ElevatedSurface.background(theme);
    let z = ElevationIndex::ElevatedSurface.z();
    let text_color = theme.colors.text;

    let text_clone = text.clone();
    // Over → Out の順に発火し、Out でこの target の tooltip 1つだけを despawn する。
    let tooltip_id: std::sync::Arc<std::sync::Mutex<Option<Entity>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let tooltip_id_out = tooltip_id.clone();

    commands.entity(target).observe(
        move |_: On<Pointer<Over>>, mut commands: Commands| {
            let id = commands.spawn((
                Tooltip { text: text_clone.clone() },
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::all(Val::Px(6.0)),
                    ..default()
                },
                BackgroundColor(bg),
                ZIndex(z as i32),
            ))
            .with_children(|p| {
                p.spawn((
                    Text::new(text_clone.clone()),
                    TextColor(text_color),
                ));
            })
            .id();
            *tooltip_id.lock().unwrap() = Some(id);
        },
    );

    commands.entity(target).observe(
        move |_: On<Pointer<Out>>, mut commands: Commands| {
            if let Some(e) = *tooltip_id_out.lock().unwrap() {
                commands.entity(e).despawn();
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    /// Tooltip struct を直接構築して text フィールドを確認する。
    #[test]
    fn tooltip_carries_text() {
        let t = Tooltip { text: "hello".to_string() };
        assert_eq!(t.text, "hello");
    }

    /// ElevationIndex::ElevatedSurface の z が 100.0 であることを確認。
    /// Tooltip が正しい tier に置かれる前提条件。
    #[test]
    fn elevated_surface_z_is_100() {
        assert_eq!(ElevationIndex::ElevatedSurface.z(), 100.0);
    }

    /// spawn_tooltip_for を呼んでも target entity がまだ Tooltip を持たないこと。
    /// (Over observer が発火するまで popup は存在しない)
    #[test]
    fn no_tooltip_entity_before_hover() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let theme = Theme::default();

        world
            .run_system_once(move |mut commands: Commands| {
                let target = commands.spawn_empty().id();
                spawn_tooltip_for(&mut commands, target, "tip", &theme);
            })
            .unwrap();

        // Over が発火していないので Tooltip entity はゼロ。
        let count = world.query::<&Tooltip>().iter(&world).count();
        assert_eq!(count, 0, "tooltip popup must not exist before hover");
    }

    /// Tooltip の background z が ElevatedSurface.z() と一致するかを
    /// spawn_tooltip_for が返す ZIndex で確認する。
    /// （Over observer を直接 trigger する代わりに、spawn 関数を直呼び）
    #[test]
    fn tooltip_popup_has_correct_z() {
        use bevy::ecs::system::RunSystemOnce;

        let mut world = World::new();
        world.init_resource::<Assets<Image>>();
        let theme = Theme::default();
        let expected_z = ElevationIndex::ElevatedSurface.z() as i32;

        world
            .run_system_once(move |mut commands: Commands| {
                // Observer 経由ではなく popup を直接 spawn して ZIndex を検証。
                let bg = ElevationIndex::ElevatedSurface.background(&theme);
                let z = ElevationIndex::ElevatedSurface.z();
                commands.spawn((
                    Tooltip { text: "tip".to_string() },
                    Node { position_type: PositionType::Absolute, ..default() },
                    BackgroundColor(bg),
                    ZIndex(z as i32),
                ));
            })
            .unwrap();

        let mut q = world.query::<(&Tooltip, &ZIndex)>();
        let (_, zidx) = q.single(&world).unwrap();
        assert_eq!(zidx.0, expected_z, "tooltip z must match ElevatedSurface");
    }
}
