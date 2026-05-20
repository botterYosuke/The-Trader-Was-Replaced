//! Phase B: 行番号 gutter。
//!
//! エディタ左に独立した `CosmicEditBuffer` (read-only) を 1 つ持ち、行番号文字列を
//! エディタと同じ `Metrics` で描画する。別 buffer にすることで行高をぴったり一致させ、
//! 行のズレを根本的に排除する (zed スキル Caveat #4)。
//!
//! ⚠️ gutter entity には **`StrategyEditorContent` を付けない** ので、highlight 系
//! (`compute_syntax_spans_system` 等) / `sync_editor_to_strategy_buffer_system` から
//! 自動的に除外される。さらに `ReadOnly` を付けることで `change_active_editor_sprite`
//! (focus on click、`Without<ReadOnly>` フィルタ) の対象外になり、クリックしても
//! フォーカスを奪わない。レンダリング自体には `TextEdit2d` が必須 (このフォークの
//! `CosmicWidgetSize::scan()` が `Has<TextEdit2d>` を要求するため) なので付ける。

use crate::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use crate::ui::strategy_editor::{
    EDITOR_TEXT_SIZE, GUTTER_WIDTH, StrategyEditorContent, editor_metrics, read_active_buffer,
};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{Attrs, AttrsOwned, Color as CosmicColor};
use bevy_cosmic_edit::prelude::*;
use bevy_cosmic_edit::{
    CosmicBackgroundColor, CosmicEditor, CosmicTextAlign, CosmicWrap, ReadOnly, ScrollEnabled,
};

/// gutter 背景色 (エディタ本体よりわずかに明るく区別する)。
const GUTTER_BG: Color = Color::srgba(0.04, 0.04, 0.07, 1.0);
/// 行番号の前景色 (控えめなグレー)。
const GUTTER_FG: CosmicColor = CosmicColor::rgb(120, 120, 140);

/// 行番号 gutter のマーカー。`region_key` で対応するエディタ (StrategyEditorId) と JOIN する。
#[derive(Component)]
pub struct LineNumberGutter {
    pub region_key: String,
}

/// gutter entity を spawn して返す (caller が content_area の子にする)。
pub fn spawn_line_number_gutter(
    commands: &mut Commands,
    font_system: &mut CosmicFontSystem,
    region_key: String,
    x: f32,
) -> Entity {
    commands
        .spawn((
            TextEdit2d,
            Sprite {
                custom_size: Some(Vec2::new(GUTTER_WIDTH, EDITOR_TEXT_SIZE.y)),
                color: Color::WHITE,
                ..default()
            },
            CosmicEditBuffer::new(font_system, editor_metrics()).with_text(
                font_system,
                "1",
                Attrs::new().color(GUTTER_FG),
            ),
            DefaultAttrs(AttrsOwned::new(Attrs::new().color(GUTTER_FG))),
            CosmicBackgroundColor(GUTTER_BG),
            // top padding をエディタ (TopLeft padding 8) と揃えて 1 行目の縦位置を一致させる。
            CosmicTextAlign::TopLeft { padding: 8 },
            CosmicWrap::InfiniteLine,
            ReadOnly,
            ScrollEnabled::Disabled,
            Transform::from_xyz(x, 0.0, 0.1),
            LineNumberGutter { region_key },
        ))
        .id()
}

/// 行数 → 右寄せ行番号テキスト (改行区切り)。幅は最大行番号の桁数に合わせる。
fn line_numbers_text(line_count: usize) -> String {
    let n = line_count.max(1);
    let width = n.to_string().len();
    (1..=n)
        .map(|i| format!("{i:>width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `StrategyFragment.source` が変わったら、対応する gutter の行番号テキストを更新する。
/// root(WindowRoot)↔gutter は region_key で JOIN (Caveat #19)。
/// `CosmicWrap::InfiniteLine` 固定により source 行 == layout 行なので行数は単純に
/// 改行数 + 1。
pub fn update_gutter_text_system(
    fragments_q: Query<
        (&StrategyEditorId, &StrategyFragment),
        (With<WindowRoot>, Changed<StrategyFragment>),
    >,
    mut gutter_q: Query<(&LineNumberGutter, &mut CosmicEditBuffer)>,
    mut font_system: ResMut<CosmicFontSystem>,
) {
    for (frag_id, fragment) in fragments_q.iter() {
        let Some((_, mut buffer)) = gutter_q
            .iter_mut()
            .find(|(g, _)| g.region_key == frag_id.region_key)
        else {
            continue;
        };
        let n = fragment.source.split('\n').count();
        let text = line_numbers_text(n);
        buffer.set_text(&mut font_system, &text, Attrs::new().color(GUTTER_FG));
        buffer.set_redraw(true);
    }
}

/// エディタの scroll 位置を gutter buffer にコピーして縦スクロールを追従させる。
/// focused なら CosmicEditor 内部 buffer、unfocused なら CosmicEditBuffer から読む (Caveat #2)。
/// `Without<StrategyEditorContent>` で editor_q と排他にして同一 `CosmicEditBuffer` への
/// 二重アクセス衝突を避ける。
pub fn sync_gutter_scroll_system(
    editor_q: Query<
        (&StrategyEditorId, Option<&CosmicEditor>, &CosmicEditBuffer),
        With<StrategyEditorContent>,
    >,
    mut gutter_q: Query<(&LineNumberGutter, &mut CosmicEditBuffer), Without<StrategyEditorContent>>,
) {
    for (gutter, mut gutter_buffer) in gutter_q.iter_mut() {
        let Some((_, editor_opt, edit_buffer)) = editor_q
            .iter()
            .find(|(id, _, _)| id.region_key == gutter.region_key)
        else {
            continue;
        };
        let scroll = read_active_buffer(editor_opt, edit_buffer, |b| b.scroll());
        if gutter_buffer.0.scroll() != scroll {
            gutter_buffer.0.set_scroll(scroll);
            gutter_buffer.0.set_redraw(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_is_just_one() {
        assert_eq!(line_numbers_text(1), "1");
    }

    #[test]
    fn zero_clamps_to_one() {
        assert_eq!(line_numbers_text(0), "1");
    }

    #[test]
    fn few_lines_no_padding() {
        assert_eq!(line_numbers_text(3), "1\n2\n3");
    }

    #[test]
    fn double_digit_right_aligns() {
        // 最大行 10 → 桁数 2 → 1 桁の番号は左に空白 1 つ。
        assert_eq!(
            line_numbers_text(10),
            " 1\n 2\n 3\n 4\n 5\n 6\n 7\n 8\n 9\n10"
        );
    }
}
