//! World-space Text2d / Sprite ベースの themed label helpers.
//!
//! Issue #46 Slice C — `buying_power.rs` / `run_result_panel.rs` /
//! `positions.rs` / `orders.rs` の手書き spawn を置き換える。

use bevy::prelude::*;

use crate::ui::theme::{LabelSize, Theme};

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// 「ラベル（左）＋ 値（右）」を world 空間に 2 つの `Text2d` entity として spawn する。
///
/// - `label_text` は `theme.colors.text_muted` で描画される。
/// - `value_text` は `theme.colors.text` で描画される。
/// - 返り値は `(label_entity, value_entity)`。
pub fn spawn_labeled_value_row(
    commands: &mut Commands,
    parent: Entity,
    label_text: impl Into<String>,
    value_text: impl Into<String>,
    label_x: f32,
    value_x: f32,
    y: f32,
    theme: &Theme,
) -> (Entity, Entity) {
    let (text_font, _line_height) = theme.typography.label_font(LabelSize::Small);

    let label_entity = commands
        .spawn((
            Text2d::new(label_text.into()),
            text_font.clone(),
            TextColor(theme.colors.text_muted),
            Transform::from_xyz(label_x, y, 0.0),
        ))
        .id();
    commands.entity(parent).add_child(label_entity);

    let value_entity = commands
        .spawn((
            Text2d::new(value_text.into()),
            text_font,
            TextColor(theme.colors.text),
            Transform::from_xyz(value_x, y, 0.0),
        ))
        .id();
    commands.entity(parent).add_child(value_entity);

    (label_entity, value_entity)
}

/// column ヘッダー文字列のリストを等間隔に world 空間へ spawn する。
///
/// - 全ヘッダーは `theme.colors.text_muted` で描画される。
/// - `x_start` から `x_step` ずつ右に並ぶ。
/// - 返り値は spawn した entity のリスト（順序は `headers` と同じ）。
pub fn spawn_table_headers(
    commands: &mut Commands,
    parent: Entity,
    headers: &[&str],
    x_start: f32,
    x_step: f32,
    y: f32,
    theme: &Theme,
) -> Vec<Entity> {
    let (text_font, _line_height) = theme.typography.label_font(LabelSize::XSmall);

    headers
        .iter()
        .enumerate()
        .map(|(i, &header)| {
            let e = commands
                .spawn((
                    Text2d::new(header),
                    text_font.clone(),
                    TextColor(theme.colors.text_muted),
                    Transform::from_xyz(x_start + x_step * i as f32, y, 0.0),
                ))
                .id();
            commands.entity(parent).add_child(e);
            e
        })
        .collect()
}

/// ヘッダーを任意 x 座標に配置するバリアント。列間隔が不等間隔なパネル用。
///
/// `headers` は `(label, x)` のスライス。`color` は `theme.colors.text_accent` などを渡す。
pub fn spawn_table_headers_at(
    commands: &mut Commands,
    parent: Entity,
    headers: &[(&str, f32)],
    y: f32,
    color: Color,
    theme: &Theme,
) -> Vec<Entity> {
    let (text_font, _line_height) = theme.typography.label_font(LabelSize::XSmall);

    headers
        .iter()
        .map(|&(label, x)| {
            let e = commands
                .spawn((
                    Text2d::new(label),
                    text_font.clone(),
                    TextColor(color),
                    Transform::from_xyz(x, y, 0.1),
                ))
                .id();
            commands.entity(parent).add_child(e);
            e
        })
        .collect()
}

/// 水平ライン divider を `Sprite` として spawn する。
///
/// - `theme.colors.border` 色、指定 `width` × 1 px の細い矩形。
pub fn spawn_divider(
    commands: &mut Commands,
    parent: Entity,
    y: f32,
    width: f32,
    theme: &Theme,
) -> Entity {
    let e = commands
        .spawn((
            Sprite {
                color: theme.colors.border,
                custom_size: Some(Vec2::new(width, 1.0)),
                ..Default::default()
            },
            Transform::from_xyz(0.0, y, 0.0),
        ))
        .id();
    commands.entity(parent).add_child(e);
    e
}

/// 小さな円形インジケーターを `Sprite`（正方形近似）として spawn する。
///
/// - `color` は呼び出し元が `theme.status.*` から選ぶ。
pub fn spawn_indicator(
    commands: &mut Commands,
    parent: Entity,
    position: Vec2,
    color: Color,
) -> Entity {
    let e = commands
        .spawn((
            Sprite {
                color,
                custom_size: Some(Vec2::splat(6.0)),
                ..Default::default()
            },
            Transform::from_xyz(position.x, position.y, 0.0),
        ))
        .id();
    commands.entity(parent).add_child(e);
    e
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// spawn_labeled_value_row が 2 つの子 entity を作り、
    /// それぞれ TextColor が text_muted / text に設定されていること（runtime-RED）。
    #[test]
    fn labeled_value_row_has_two_children_with_correct_colors() {
        let mut world = World::new();
        let theme = Theme::default();
        world.insert_resource(theme.clone());

        let parent = world.spawn(Transform::default()).id();

        let (label_e, value_e) = world
            .run_system_once(
                move |mut commands: Commands, theme: Res<Theme>| {
                    spawn_labeled_value_row(
                        &mut commands,
                        parent,
                        "資産",
                        "1,000,000",
                        -100.0,
                        60.0,
                        0.0,
                        &theme,
                    )
                },
            )
            .unwrap();

        let label_color = world.get::<TextColor>(label_e).expect("label TextColor missing");
        let value_color = world.get::<TextColor>(value_e).expect("value TextColor missing");

        assert_eq!(
            label_color.0,
            theme.colors.text_muted,
            "label should use text_muted"
        );
        assert_eq!(
            value_color.0,
            theme.colors.text,
            "value should use text"
        );
    }
}
