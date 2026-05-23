//! Phase B: スクロールバー。
//!
//! エディタ右に縦の track + thumb (`Sprite`) を置く。thumb は `target_editor` を carry し
//! (multi-spawn で複数 thumb が並ぶため必須、Caveat #13)、`Pointer<Drag>` で縦ドラッグすると
//! 対象エディタの `Scroll::line` を逆換算して動かす。マウスホイールは cosmic_edit 既定が
//! 効くので追加不要。thumb の高さ・位置はビューポート行数 / 総行数 / scroll 位置から毎フレーム算出。

use crate::ui::strategy_editor::{
    EDITOR_TEXT_SIZE, SCROLLBAR_WIDTH, StrategyEditorContent, editor_metrics, read_active_buffer,
};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Edit, Scroll};
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicEditor};

/// scrollbar track 背景色。
const SCROLLBAR_TRACK_BG: Color = Color::srgba(0.06, 0.06, 0.10, 0.6);
/// scrollbar thumb 色。
const SCROLLBAR_THUMB_FG: Color = Color::srgba(0.40, 0.40, 0.50, 0.9);

/// scrollbar thumb のマーカー。`target_editor` で操作対象エディタを保持する (Caveat #13)。
#[derive(Component)]
pub struct EditorScrollThumb {
    pub target_editor: Entity,
}

/// scrollbar track のマーカー (thumb の親)。
#[derive(Component)]
pub struct EditorScrollbarTrack;

/// thumb の幾何計算に必要な状態をまとめたもの。総行数 / 表示行数 / track 高から
/// thumb 高を導出して保持し、位置算出 (`center_y`) とドラッグ逆算 (`line_from_drag`) を提供する。
struct ThumbMetrics {
    total: usize,
    viewport: usize,
    track_h: f32,
    thumb_h: f32,
}

impl ThumbMetrics {
    fn new(total: usize, viewport: usize, track_h: f32) -> Self {
        let thumb_h = thumb_height(total, viewport, track_h);
        Self {
            total,
            viewport,
            track_h,
            thumb_h,
        }
    }

    /// 現在の editor 状態から組み立てる。track_h は実際の track sprite 高を渡す。
    fn from_editor_with_height(
        editor: Option<&CosmicEditor>,
        buffer: &CosmicEditBuffer,
        track_h: f32,
    ) -> (Self, usize) {
        let viewport = viewport_lines(track_h, editor_metrics().line_height);
        let (total, scroll_line) =
            read_active_buffer(editor, buffer, |b| (b.lines.len().max(1), b.scroll().line));
        (Self::new(total, viewport, track_h), scroll_line)
    }

    /// スクロール可能行数 (0 なら全行表示中)。
    fn max_line(&self) -> usize {
        self.total.saturating_sub(self.viewport)
    }

    /// thumb の中心 y (track 中心基準。track は中央配置なので子の Transform.y と一致)。
    /// scroll 先頭で上端、末尾で下端。
    fn center_y(&self, scroll_line: usize) -> f32 {
        let denom = self.max_line();
        let fraction = if denom == 0 {
            0.0
        } else {
            (scroll_line.min(denom) as f32) / (denom as f32)
        };
        let travel = self.track_h - self.thumb_h;
        (self.track_h / 2.0 - self.thumb_h / 2.0) - fraction * travel
    }

    /// drag (world px) から新しい scroll 行を逆算する。下方向ドラッグ (+y screen) で行が増える。
    fn line_from_drag(&self, current_line: usize, delta_world_y: f32) -> usize {
        let denom = self.max_line();
        if denom == 0 {
            return 0;
        }
        let travel = (self.track_h - self.thumb_h).max(1.0);
        let lines_per_px = denom as f32 / travel;
        let new = (current_line as f32 + delta_world_y * lines_per_px).round();
        new.clamp(0.0, denom as f32) as usize
    }
}

/// scrollbar (track + thumb child) を spawn して track entity を返す。
/// caller が track を content_area の子にする。thumb は track の子。
pub fn spawn_editor_scrollbar(commands: &mut Commands, target_editor: Entity, x: f32) -> Entity {
    let track_h = EDITOR_TEXT_SIZE.y;
    let track = commands
        .spawn((
            Sprite {
                custom_size: Some(Vec2::new(SCROLLBAR_WIDTH, track_h)),
                color: SCROLLBAR_TRACK_BG,
                ..default()
            },
            Transform::from_xyz(x, 0.0, 0.1),
            EditorScrollbarTrack,
        ))
        .id();

    let thumb = commands
        .spawn((
            Sprite {
                custom_size: Some(Vec2::new(SCROLLBAR_WIDTH, track_h)),
                color: SCROLLBAR_THUMB_FG,
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 0.1),
            EditorScrollThumb { target_editor },
        ))
        .observe(
            |drag: Trigger<Pointer<Drag>>,
             thumb_q: Query<(&EditorScrollThumb, &Parent)>,
             track_q: Query<&Sprite, (With<EditorScrollbarTrack>, Without<EditorScrollThumb>)>,
             mut editor_q: Query<
                (Option<&mut CosmicEditor>, &mut CosmicEditBuffer),
                With<StrategyEditorContent>,
            >,
             camera_q: Query<&OrthographicProjection, With<Camera2d>>| {
                let Ok((thumb, parent)) = thumb_q.get(drag.entity()) else {
                    return;
                };
                let Ok((mut editor_opt, mut buffer)) = editor_q.get_mut(thumb.target_editor) else {
                    return;
                };
                let scale = camera_q.get_single().map(|p| p.scale).unwrap_or(1.0);
                let track_h = track_q
                    .get(parent.get())
                    .ok()
                    .and_then(|s| s.custom_size)
                    .map(|s| s.y)
                    .unwrap_or(EDITOR_TEXT_SIZE.y);
                let (metrics, current_line) =
                    ThumbMetrics::from_editor_with_height(editor_opt.as_deref(), &buffer, track_h);
                let delta_world_y = drag.event().delta.y * scale;
                let new_scroll = Scroll {
                    line: metrics.line_from_drag(current_line, delta_world_y),
                    vertical: 0.0,
                    horizontal: 0.0,
                };
                if let Some(editor) = editor_opt.as_deref_mut() {
                    editor.with_buffer_mut(|b| b.set_scroll(new_scroll));
                    editor.set_redraw(true);
                } else {
                    buffer.0.set_scroll(new_scroll);
                    buffer.0.set_redraw(true);
                }
            },
        )
        .id();

    commands.entity(track).add_child(thumb);
    track
}

/// track 高 / 行高 から表示可能行数を求める (最低 1)。
fn viewport_lines(track_h: f32, line_height: f32) -> usize {
    if line_height <= 0.0 {
        return 1;
    }
    ((track_h / line_height).floor() as usize).max(1)
}

/// thumb の高さ。`(viewport / total).clamp(0.05, 1.0) * track_h`。
fn thumb_height(total: usize, viewport: usize, track_h: f32) -> f32 {
    let ratio = (viewport as f32 / total.max(1) as f32).clamp(0.05, 1.0);
    ratio * track_h
}

/// thumb の高さ・縦位置を target editor の scroll 状態から更新する。
/// 差分書き込みで change detection の無駄発火を防ぐ (規約 2)。
pub fn update_scrollbar_thumb_system(
    editor_q: Query<(Option<&CosmicEditor>, &CosmicEditBuffer), With<StrategyEditorContent>>,
    mut thumb_q: Query<(&EditorScrollThumb, &mut Sprite, &mut Transform, &Parent)>,
    track_q: Query<&Sprite, (With<EditorScrollbarTrack>, Without<EditorScrollThumb>)>,
) {
    for (thumb, mut sprite, mut tf, parent) in thumb_q.iter_mut() {
        let Ok((editor_opt, buffer)) = editor_q.get(thumb.target_editor) else {
            continue;
        };
        let track_h = track_q
            .get(parent.get())
            .ok()
            .and_then(|s| s.custom_size)
            .map(|s| s.y)
            .unwrap_or(EDITOR_TEXT_SIZE.y);
        let (metrics, scroll_line) = ThumbMetrics::from_editor_with_height(editor_opt, buffer, track_h);
        let center_y = metrics.center_y(scroll_line);

        if sprite.custom_size.map(|s| s.y) != Some(metrics.thumb_h) {
            sprite.custom_size = Some(Vec2::new(SCROLLBAR_WIDTH, metrics.thumb_h));
        }
        if (tf.translation.y - center_y).abs() > 0.01 {
            tf.translation.y = center_y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(total: usize, viewport: usize) -> ThumbMetrics {
        ThumbMetrics::new(total, viewport, 320.0)
    }

    #[test]
    fn viewport_floor() {
        assert_eq!(viewport_lines(320.0, 18.0), 17);
        assert_eq!(viewport_lines(36.0, 18.0), 2);
        assert_eq!(viewport_lines(10.0, 0.0), 1);
    }

    #[test]
    fn thumb_height_proportional() {
        // 100 行 / 17 表示 → 0.17 * 320 = 54.4
        assert!((metrics(100, 17).thumb_h - 54.4).abs() < 0.01);
    }

    #[test]
    fn thumb_height_fills_when_all_visible() {
        // total <= viewport → ratio 1.0 → track 全体
        assert!((metrics(10, 17).thumb_h - 320.0).abs() < 0.01);
    }

    #[test]
    fn thumb_height_min_clamp() {
        // 巨大ファイルでも 5% 未満にはしない
        assert!((metrics(100_000, 17).thumb_h - 16.0).abs() < 0.01);
    }

    #[test]
    fn thumb_top_when_scroll_zero() {
        let m = metrics(100, 17);
        let y = m.center_y(0);
        assert!((y - (160.0 - m.thumb_h / 2.0)).abs() < 0.01);
    }

    #[test]
    fn thumb_bottom_when_scroll_max() {
        let m = metrics(100, 17);
        let y = m.center_y(m.max_line());
        assert!((y + (160.0 - m.thumb_h / 2.0)).abs() < 0.01); // 下端 = -上端
    }

    #[test]
    fn thumb_center_no_scroll_when_all_visible() {
        let m = metrics(10, 17); // thumb_h = 320 (full)
        assert!(m.center_y(0).abs() < 0.01); // 中央 (travel 0)
    }

    #[test]
    fn drag_full_travel_reaches_max() {
        let m = metrics(100, 17);
        let travel = 320.0 - m.thumb_h;
        assert_eq!(m.line_from_drag(0, travel), 83); // denom = 100 - 17
    }

    #[test]
    fn drag_clamps_at_top() {
        let m = metrics(100, 17);
        assert_eq!(m.line_from_drag(5, -10_000.0), 0);
    }

    #[test]
    fn drag_noop_when_all_visible() {
        let m = metrics(10, 17);
        assert_eq!(m.line_from_drag(0, 500.0), 0);
    }
}
