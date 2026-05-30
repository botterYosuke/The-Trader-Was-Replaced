//! Popover component — anchor-relative popup dismissed by Esc (Issue #46 Slice F).
//!
//! `spawn_popover` は backdrop なし・`ModalLayer` に push する「軽い版モーダル」。
//! Esc dismiss は既存の `modal_layer_esc_system` が担当する（専用 esc system 不要）。
//! z は `ElevationIndex::ElevatedSurface`（100.0）、dismiss_priority = 100。

use crate::ui::component::modal_layer::{ActiveModal, DismissDecision, ModalHandle, ModalLayer};
use crate::ui::theme::{ElevationIndex, Theme};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Skeleton
// ---------------------------------------------------------------------------

/// Popover の宣言的仕様。`spawn_popover` に渡す。
pub struct PopoverSkeleton {
    /// Popover カードの幅 (px)。
    pub width: f32,
    /// スクリーン左上原点の anchor 座標。Popover の左上がここに来る。
    pub anchor_pos: Vec2,
}

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// Popover を spawn し、`ModalLayer` に push する。
///
/// - backdrop なし（root 自体は `Display::Block` で即表示）。
/// - `dismiss_priority` = 100（既存モーダルより低い）。
/// - Esc は `modal_layer_esc_system` が担当するため専用 system 不要。
/// - 戻り値 `ModalHandle.root` が Popover の root entity、`.card` がコンテンツを置く surface。
pub fn spawn_popover(
    commands: &mut Commands,
    layer: &mut ModalLayer,
    theme: &Theme,
    skeleton: PopoverSkeleton,
) -> ModalHandle {
    let bg = ElevationIndex::ElevatedSurface.background(theme);
    let z = ElevationIndex::ElevatedSurface.z() as i32;

    let card = commands
        .spawn((
            Node {
                width: Val::Px(skeleton.width),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(bg),
            ElevationIndex::ElevatedSurface,
        ))
        .id();

    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(skeleton.anchor_pos.x),
                top: Val::Px(skeleton.anchor_pos.y),
                ..default()
            },
            GlobalZIndex(z),
            Name::new("Popover"),
        ))
        .add_child(card)
        .id();

    layer.push(ActiveModal {
        root,
        backdrop: root, // backdrop なし → root を共用
        previous_focus: None,
        dismiss_priority: 100,
        on_before_dismiss: || DismissDecision::Dismiss,
    });

    ModalHandle { root, card }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::component::modal_layer::ModalLayer;
    use crate::ui::theme::Theme;
    use bevy::ecs::system::RunSystemOnce;

    /// spawn_popover が ModalLayer に push する（stack 長が 1 になる）。
    #[test]
    fn popover_push_to_modal_layer() {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        world.insert_resource(ModalLayer::default());

        world
            .run_system_once(
                |mut commands: Commands, theme: Res<Theme>, mut layer: ResMut<ModalLayer>| {
                    spawn_popover(
                        &mut commands,
                        &mut layer,
                        &theme,
                        PopoverSkeleton { width: 200.0, anchor_pos: Vec2::new(10.0, 20.0) },
                    );
                },
            )
            .unwrap();

        let layer = world.resource::<ModalLayer>();
        assert_eq!(layer.stack.len(), 1, "popover は ModalLayer に push される");
    }

    /// dismiss_priority が 100 であること。
    #[test]
    fn popover_dismiss_priority_is_100() {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        world.insert_resource(ModalLayer::default());

        world
            .run_system_once(
                |mut commands: Commands, theme: Res<Theme>, mut layer: ResMut<ModalLayer>| {
                    spawn_popover(
                        &mut commands,
                        &mut layer,
                        &theme,
                        PopoverSkeleton { width: 200.0, anchor_pos: Vec2::ZERO },
                    );
                },
            )
            .unwrap();

        let layer = world.resource::<ModalLayer>();
        assert_eq!(layer.stack[0].dismiss_priority, 100);
    }

    /// ElevatedSurface z が 100.0（Popover が正しい tier に置かれる前提）。
    #[test]
    fn popover_z_is_elevated_surface() {
        assert_eq!(ElevationIndex::ElevatedSurface.z(), 100.0);
    }

    /// anchor_pos が root の left/top に反映される。
    #[test]
    fn popover_anchor_pos_applied_to_node() {
        let mut world = World::new();
        world.insert_resource(Theme::default());
        world.insert_resource(ModalLayer::default());

        let handle = world
            .run_system_once(
                |mut commands: Commands, theme: Res<Theme>, mut layer: ResMut<ModalLayer>| {
                    spawn_popover(
                        &mut commands,
                        &mut layer,
                        &theme,
                        PopoverSkeleton { width: 300.0, anchor_pos: Vec2::new(50.0, 80.0) },
                    )
                },
            )
            .unwrap();

        let node = world.entity(handle.root).get::<Node>().unwrap();
        assert_eq!(node.left, Val::Px(50.0));
        assert_eq!(node.top, Val::Px(80.0));
    }
}
