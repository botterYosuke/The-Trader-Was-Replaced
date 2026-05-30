//! ContextMenu component — right-click popup dismissed by Esc (Issue #46 Slice F).
//!
//! `ContextMenuLayer` Resource が単一のメニューを管理する。
//! 2 つ目を開くと 1 つ目が despawn される（single-instance）。
//! z は `ElevationIndex::ElevatedSurface`（100.0）。

use crate::ui::theme::{ElevationIndex, Theme};
use bevy::prelude::*;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// メニューの 1 エントリ。
#[derive(Clone, Debug)]
pub struct ContextMenuEntry {
    pub label: String,
}

/// メニュー全体の設定（entries リスト）。
#[derive(Clone, Debug)]
pub struct ContextMenuConfig {
    pub entries: Vec<ContextMenuEntry>,
}

// ---------------------------------------------------------------------------
// Marker component
// ---------------------------------------------------------------------------

/// ContextMenu popup の root entity に付くマーカー。
#[derive(Component, Debug)]
pub struct ContextMenuMarker;

// ---------------------------------------------------------------------------
// Resource
// ---------------------------------------------------------------------------

/// 現在開いている ContextMenu の状態。
/// 2 つ目を開くと 1 つ目の entity を返す → 呼び出し側が despawn できる。
#[derive(Resource, Default, Debug)]
pub struct ContextMenuLayer {
    /// 現在表示中の popup entity。
    pub current: Option<Entity>,
}

impl ContextMenuLayer {
    /// 新しい entity を登録し、置き換えられる旧 entity を返す。
    /// 呼び出し側は戻り値を `commands.entity(old).despawn()` すること。
    pub fn open(&mut self, entity: Entity) -> Option<Entity> {
        let old = self.current.take();
        self.current = Some(entity);
        old
    }

    /// メニューを閉じ、閉じた entity を返す。
    pub fn close(&mut self) -> Option<Entity> {
        self.current.take()
    }

    pub fn is_open(&self) -> bool {
        self.current.is_some()
    }
}

// ---------------------------------------------------------------------------
// Spawn helper
// ---------------------------------------------------------------------------

/// `pos`（スクリーン左上原点）にコンテキストメニューを spawn する。
/// `ContextMenuLayer` の旧 entity は自動 despawn される。
/// 戻り値は新しい root entity。
pub fn spawn_context_menu(
    commands: &mut Commands,
    layer: &mut ContextMenuLayer,
    theme: &Theme,
    pos: Vec2,
    entries: &[ContextMenuEntry],
) -> Entity {
    let bg = ElevationIndex::ElevatedSurface.background(theme);
    let z = ElevationIndex::ElevatedSurface.z() as i32;
    let text_color = theme.colors.text;

    let root = commands
        .spawn((
            ContextMenuMarker,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pos.x),
                top: Val::Px(pos.y),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(bg),
            GlobalZIndex(z),
        ))
        .with_children(|parent| {
            for entry in entries {
                let label = entry.label.clone();
                parent.spawn((
                    Text::new(label),
                    TextColor(text_color),
                ));
            }
        })
        .id();

    if let Some(old) = layer.open(root) {
        commands.entity(old).despawn();
    }

    root
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Esc でコンテキストメニューを閉じる。
pub fn context_menu_esc_system(
    keys: Res<ButtonInput<KeyCode>>,
    mut layer: ResMut<ContextMenuLayer>,
    mut commands: Commands,
) {
    if !layer.is_open() {
        return;
    }
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if let Some(entity) = layer.close() {
        commands.entity(entity).despawn();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn make_entries(labels: &[&str]) -> Vec<ContextMenuEntry> {
        labels.iter().map(|s| ContextMenuEntry { label: s.to_string() }).collect()
    }

    /// ContextMenuEntry / Config の構造体が正しいフィールドを持つ。
    #[test]
    fn context_menu_entry_holds_label() {
        let entry = ContextMenuEntry { label: "取消".to_string() };
        assert_eq!(entry.label, "取消");

        let config = ContextMenuConfig {
            entries: make_entries(&["取消", "訂正"]),
        };
        assert_eq!(config.entries.len(), 2);
    }

    /// ElevatedSurface の z が 100.0 であること（ContextMenu が正しい tier に置かれる前提）。
    #[test]
    fn context_menu_z_is_elevated_surface() {
        assert_eq!(ElevationIndex::ElevatedSurface.z(), 100.0);
    }

    /// ContextMenuLayer::open が旧 entity を返し、新 entity を登録する。
    #[test]
    fn context_menu_layer_open_returns_old() {
        let mut layer = ContextMenuLayer::default();

        let e1 = Entity::from_raw_u32(1).unwrap();
        let e2 = Entity::from_raw_u32(2).unwrap();

        let old = layer.open(e1);
        assert!(old.is_none(), "初回は旧 entity なし");
        assert!(layer.is_open());

        let old2 = layer.open(e2);
        assert_eq!(old2, Some(e1), "2 回目は旧 entity e1 が返る");
        assert_eq!(layer.current, Some(e2));
    }

    /// ContextMenuLayer::close が entity を返し、is_open が false になる。
    #[test]
    fn context_menu_layer_close_clears() {
        let mut layer = ContextMenuLayer::default();
        let e = Entity::from_raw_u32(10).unwrap();
        layer.open(e);
        assert!(layer.is_open());

        let closed = layer.close();
        assert_eq!(closed, Some(e));
        assert!(!layer.is_open());
    }
}
