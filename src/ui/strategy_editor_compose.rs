use crate::ui::strategy_editor::StrategyEditorContent;
use crate::ui::strategy_editor_highlight::{
    BracketSpans, FindMatchSpans, MatchSpan, SpanStyle, SyntaxSpans,
};
use bevy::prelude::*;
use bevy_cosmic_edit::cosmic_text::{self, Attrs, AttrsList, Color, Edit};
use bevy_cosmic_edit::{CosmicEditBuffer, CosmicEditor, DefaultAttrs};

/// Find マッチ (非カレント) の前景色。
pub const FIND_MATCH_FG: Color = Color::rgba(255, 180, 80, 255);
/// Find マッチ (カレント) の前景色。
pub const FIND_CURRENT_MATCH_FG: Color = Color::rgba(255, 240, 0, 255);
/// bracket 対応ペアの前景色。
pub const BRACKET_MATCH_FG: Color = Color::rgba(80, 220, 220, 255);

/// 1 行分の AttrsList を組み立てる。span は固定順で適用する:
/// default → syntax → find → current_find → bracket。
/// cosmic_text の `add_span` は `RangeMap::insert` で重なり部分を上書きするため、
/// 後から add した span が勝つ (= bracket > current_find > find > syntax > default)。
pub fn compose_attrs_for_line(
    base: cosmic_text::Attrs,
    syntax: &[SpanStyle],
    find: &[SpanStyle],
    current_find: Option<&SpanStyle>,
    bracket: &[SpanStyle],
) -> cosmic_text::AttrsList {
    let mut list = AttrsList::new(base);

    for span in syntax {
        list.add_span(span.byte_range.clone(), apply_span(base, span));
    }
    for span in find {
        list.add_span(span.byte_range.clone(), apply_span(base, span));
    }
    if let Some(span) = current_find {
        list.add_span(span.byte_range.clone(), apply_span(base, span));
    }
    for span in bracket {
        list.add_span(span.byte_range.clone(), apply_span(base, span));
    }

    list
}

/// span の fg を base に重ねた Attrs を返す。
/// fg が None の span は base の色をそのまま使う。
fn apply_span<'a>(base: Attrs<'a>, span: &SpanStyle) -> Attrs<'a> {
    match span.fg {
        Some(color) => base.color(color),
        None => base,
    }
}

/// Find マッチ 1 件を SpanStyle に変換する。
/// is_current なら FIND_CURRENT_MATCH_FG、それ以外は FIND_MATCH_FG。
fn span_from_match(m: &MatchSpan, is_current: bool) -> SpanStyle {
    SpanStyle {
        byte_range: m.byte_range.clone(),
        fg: Some(if is_current {
            FIND_CURRENT_MATCH_FG
        } else {
            FIND_MATCH_FG
        }),
    }
}

/// 3 つの span source (syntax / find / bracket) の Ref を監視し、変化したフレームだけ
/// dirty 行の AttrsList を再構築して `BufferLine::set_attrs_list` で適用する。
/// これが `set_attrs_list` を呼ぶ唯一の場所 (Caveat #1)。
/// dirty 行集合:
///   - syntax 変化 → 全 syntax.lines の index
///   - find 変化   → 各 matches[].line + prev_match_lines
///   - bracket 変化 → prev_pair / pair の両エントリの行
/// 適用後、redraw を立てる: editor があれば editor 側 buffer、無ければ buffer 単体。
pub fn apply_highlight_layers_system(
    mut editor_q: Query<
        (
            &mut CosmicEditBuffer,
            Option<&mut CosmicEditor>,
            Ref<SyntaxSpans>,
            Ref<FindMatchSpans>,
            Ref<BracketSpans>,
            &DefaultAttrs,
        ),
        With<StrategyEditorContent>,
    >,
) {
    for (mut buffer, editor_opt, syntax, find, bracket, default_attrs) in editor_q.iter_mut() {
        // どの span source も変化していなければ何もしない。
        if !syntax.is_changed() && !find.is_changed() && !bracket.is_changed() {
            continue;
        }

        // dirty 行集合を構築する。
        let mut dirty: std::collections::HashSet<usize> = std::collections::HashSet::new();

        if syntax.is_changed() {
            for i in 0..syntax.lines.len() {
                dirty.insert(i);
            }
        }
        if find.is_changed() {
            for m in &find.matches {
                dirty.insert(m.line);
            }
            for &line in &find.prev_match_lines {
                dirty.insert(line);
            }
        }
        if bracket.is_changed() {
            if let Some(pair) = &bracket.pair {
                dirty.insert(pair[0].0);
                dirty.insert(pair[1].0);
            }
            if let Some(prev) = &bracket.prev_pair {
                dirty.insert(prev[0].0);
                dirty.insert(prev[1].0);
            }
        }

        if dirty.is_empty() {
            continue;
        }

        let base = default_attrs.0.as_attrs();

        // dirty 行ごとに AttrsList を再構築して適用する。
        // 行データの取得には editor 側 buffer を優先 (editor があれば編集中のテキストはそちら)。
        let empty: Vec<SpanStyle> = Vec::new();
        let apply = |b: &mut cosmic_text::Buffer| {
            for &i in &dirty {
                if i >= b.lines.len() {
                    continue;
                }

                // syntax span (この行) — 無ければ空。
                let syntax_spans: &[SpanStyle] = syntax.lines.get(i).unwrap_or(&empty);

                // find span (この行, current を除く) と current_find。
                let mut find_spans: Vec<SpanStyle> = Vec::new();
                let mut current_find: Option<SpanStyle> = None;
                for (idx, m) in find.matches.iter().enumerate() {
                    if m.line != i {
                        continue;
                    }
                    let is_current = find.current_idx == Some(idx);
                    if is_current {
                        current_find = Some(span_from_match(m, true));
                    } else {
                        find_spans.push(span_from_match(m, false));
                    }
                }

                // bracket span (この行のペアエントリ)。
                let mut bracket_spans: Vec<SpanStyle> = Vec::new();
                if let Some(pair) = &bracket.pair {
                    for (line, range) in pair.iter() {
                        if *line == i {
                            bracket_spans.push(SpanStyle {
                                byte_range: range.clone(),
                                fg: Some(BRACKET_MATCH_FG),
                            });
                        }
                    }
                }

                let attrs_list = compose_attrs_for_line(
                    base,
                    syntax_spans,
                    &find_spans,
                    current_find.as_ref(),
                    &bracket_spans,
                );

                b.lines[i].set_attrs_list(attrs_list);
            }
        };

        if let Some(mut editor) = editor_opt {
            editor.with_buffer_mut(|b| {
                apply(b);
                b.set_redraw(true);
            });
            editor.set_redraw(true);
        } else {
            apply(&mut buffer.0);
            buffer.0.set_redraw(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// base には色を載せず None のままにして、span 由来の色だけを検証する。
    fn base_attrs() -> Attrs<'static> {
        Attrs::new()
    }

    fn syntax_color() -> Color {
        Color::rgba(10, 20, 30, 255)
    }

    fn syntax_span(range: std::ops::Range<usize>) -> SpanStyle {
        SpanStyle {
            byte_range: range,
            fg: Some(syntax_color()),
        }
    }

    fn find_span(range: std::ops::Range<usize>) -> SpanStyle {
        SpanStyle {
            byte_range: range,
            fg: Some(FIND_MATCH_FG),
        }
    }

    fn bracket_span(range: std::ops::Range<usize>) -> SpanStyle {
        SpanStyle {
            byte_range: range,
            fg: Some(BRACKET_MATCH_FG),
        }
    }

    /// (1) 全 span 空 → spans は空で、どの index も base (color_opt None) を返す。
    #[test]
    fn empty_spans_yield_base_only() {
        let base = base_attrs();
        let list = compose_attrs_for_line(base, &[], &[], None, &[]);

        assert!(list.spans().is_empty(), "no spans expected");
        assert_eq!(list.get_span(0).color_opt, None);
        assert_eq!(list.get_span(5).color_opt, base.color_opt);
    }

    /// (2) syntax のみ → その range だけ syntax 色、範囲外は base。
    #[test]
    fn syntax_only_colors_its_range() {
        let base = base_attrs();
        let list = compose_attrs_for_line(base, &[syntax_span(2..5)], &[], None, &[]);

        assert_eq!(list.get_span(3).color_opt, Some(syntax_color()));
        // 範囲外は base (None)。
        assert_eq!(list.get_span(0).color_opt, None);
        assert_eq!(list.get_span(6).color_opt, None);
    }

    /// (3) syntax と find が同一 range で重なる → 後から add した find が勝つ。
    #[test]
    fn find_overrides_syntax_on_overlap() {
        let base = base_attrs();
        let list =
            compose_attrs_for_line(base, &[syntax_span(2..5)], &[find_span(2..5)], None, &[]);

        assert_eq!(list.get_span(3).color_opt, Some(FIND_MATCH_FG));
    }

    /// (4) syntax + find + bracket が全部同一 range で重なる → bracket が勝つ。
    #[test]
    fn bracket_wins_over_all() {
        let base = base_attrs();
        let list = compose_attrs_for_line(
            base,
            &[syntax_span(2..5)],
            &[find_span(2..5)],
            None,
            &[bracket_span(2..5)],
        );

        assert_eq!(list.get_span(3).color_opt, Some(BRACKET_MATCH_FG));
    }

    /// (5) find と current_find が同一位置 → current_find (後 add) が勝つ。
    #[test]
    fn current_find_wins_over_find() {
        let base = base_attrs();
        let m = MatchSpan {
            line: 0,
            byte_range: 2..5,
        };
        let find = span_from_match(&m, false);
        let current = span_from_match(&m, true);

        let list = compose_attrs_for_line(base, &[find], &[], Some(&current), &[]);

        assert_eq!(list.get_span(3).color_opt, Some(FIND_CURRENT_MATCH_FG));
    }
}
