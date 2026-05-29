use crate::ui::components::{StrategyEditorId, StrategyFragment, WindowRoot};
use crate::ui::strategy_editor::StrategyEditorContent;
use bevy::prelude::*;
use bevy_cosmic_edit::CosmicEditor;
use bevy_cosmic_edit::cosmic_text::{self, Edit};
use bevy_cosmic_edit::prelude::FocusedWidget;
use syntect::easy::HighlightLines;
use syntect::util::LinesWithEndings;

/// syntect の SyntaxSet / Theme / python SyntaxReference を保持する resource。
/// `SyntaxSet` / `Theme` は Send + Sync なので通常の Resource で OK。
#[derive(Resource)]
pub struct SyntectHighlighter {
    pub syntax_set: syntect::parsing::SyntaxSet,
    pub theme: syntect::highlighting::Theme,
    pub python_syntax: syntect::parsing::SyntaxReference,
}

/// Startup で 1 回だけ。load_defaults_newlines は数十〜百 ms かかるので毎フレーム禁止。
pub fn init_syntect_highlighter(mut commands: Commands) {
    let syntax_set = syntect::parsing::SyntaxSet::load_defaults_newlines();
    let theme_set = syntect::highlighting::ThemeSet::load_defaults();
    let theme = theme_set
        .themes
        .get("base16-mocha.dark")
        .expect("syntect ThemeSet::load_defaults() must include base16-mocha.dark")
        .clone();
    let python_syntax = syntax_set
        .find_syntax_by_extension("py")
        .expect("syntect default set includes python")
        .clone();
    commands.insert_resource(SyntectHighlighter {
        syntax_set,
        theme,
        python_syntax,
    });
}

/// 1 行内の 1 span 分のスタイル。foreground のみ持つ (cosmic_text::Attrs に背景色 API が無いため)。
pub struct SpanStyle {
    pub byte_range: std::ops::Range<usize>,
    pub fg: Option<cosmic_text::Color>,
}

// Slice 5 (#50): MatchSpan / FindMatchSpans の定義は `strategy_editor_find` 側に move 済み。
// 旧 path 経由の参照を壊さないよう shim re-export だけ残す。Slice 6 で _highlight.rs ごと削除予定。
pub use crate::ui::strategy_editor_find::{FindMatchSpans, MatchSpan};

/// syntect トークナイズ結果。`compute_syntax_spans_system` が書き込む。
#[derive(Component, Default)]
pub struct SyntaxSpans {
    pub lines: Vec<Vec<SpanStyle>>,
}

/// bracket 対応ペア。`compute_bracket_spans_system` が書き込む。
#[derive(Component, Default)]
pub struct BracketSpans {
    pub pair: Option<[(usize, std::ops::Range<usize>); 2]>,
    pub prev_pair: Option<[(usize, std::ops::Range<usize>); 2]>,
}

/// syntect の 1 行分 `(Style, &str)` 列を、行内 byte offset を累積しながら
/// `Vec<SpanStyle>` に変換する。
fn convert_syntect_ranges_to_spans(
    ranges: &[(syntect::highlighting::Style, &str)],
) -> Vec<SpanStyle> {
    let mut spans = Vec::with_capacity(ranges.len());
    let mut offset = 0usize;
    for (style, text) in ranges {
        let len = text.len();
        if len > 0 {
            let c = style.foreground;
            spans.push(SpanStyle {
                byte_range: offset..offset + len,
                fg: Some(cosmic_text::Color::rgba(c.r, c.g, c.b, c.a)),
            });
        }
        offset += len;
    }
    spans
}

/// StrategyFragment.source が変わったら syntect でトークナイズし直し、
/// 対応する editor の SyntaxSpans に行ごとの SpanStyle を書き込む。
/// root(WindowRoot)↔editor(StrategyEditorContent) は region_key で JOIN する (Caveat #19)。
pub fn compute_syntax_spans_system(
    highlighter: Res<SyntectHighlighter>,
    fragments_q: Query<
        (&StrategyEditorId, &StrategyFragment),
        (With<WindowRoot>, Changed<StrategyFragment>),
    >,
    mut editor_q: Query<(&StrategyEditorId, &mut SyntaxSpans), With<StrategyEditorContent>>,
) {
    for (frag_id, fragment) in fragments_q.iter() {
        let Some((_, mut spans)) = editor_q
            .iter_mut()
            .find(|(editor_id, _)| editor_id.region_key == frag_id.region_key)
        else {
            continue;
        };

        let mut highlighter_lines =
            HighlightLines::new(&highlighter.python_syntax, &highlighter.theme);
        let mut new_lines: Vec<Vec<SpanStyle>> = Vec::new();
        for line in LinesWithEndings::from(&fragment.source) {
            let ranges = highlighter_lines
                .highlight_line(line, &highlighter.syntax_set)
                .unwrap_or_default();
            new_lines.push(convert_syntect_ranges_to_spans(&ranges));
        }

        spans.lines = new_lines;
    }
}

/// focus 中の editor のカーソル周辺で対応する括弧ペアを探し、BracketSpans に書く。
/// カーソルが前フレームと同じならスキップ (Local キャッシュ)。
/// pair をセットする前に必ず prev_pair = pair.take() を行い、apply 側が
/// 「前フレームのペア行」も dirty に含められるようにする。
pub fn compute_bracket_spans_system(
    focused: Res<FocusedWidget>,
    mut editor_q: Query<(Entity, &CosmicEditor, &mut BracketSpans), With<StrategyEditorContent>>,
    mut last_cursor: Local<Option<(Entity, cosmic_text::Cursor)>>,
) {
    let Some(focused_entity) = focused.0 else {
        return;
    };
    let Ok((entity, editor, mut bracket)) = editor_q.get_mut(focused_entity) else {
        return;
    };

    let cursor = editor.cursor();
    if *last_cursor == Some((entity, cursor)) {
        return;
    }
    *last_cursor = Some((entity, cursor));

    let new_pair = editor.with_buffer(|buffer| find_bracket_pair(buffer, cursor));

    bracket.prev_pair = bracket.pair.take();
    bracket.pair = new_pair;
}

/// カーソル直前 / 直後の括弧に対応する相方を、同一バッファ内で前後スキャンして探す。
/// 見つかれば `[(line, byte_range); 2]` (open, close の順) を返す。
/// スキャンは合計 ~4096 文字で打ち切る (巨大ファイルの毎フレームコスト上限)。
fn find_bracket_pair(
    buffer: &cosmic_text::Buffer,
    cursor: cosmic_text::Cursor,
) -> Option<[(usize, std::ops::Range<usize>); 2]> {
    const SCAN_CAP: usize = 4096;

    let line_count = buffer.lines.len();
    if cursor.line >= line_count {
        return None;
    }

    // カーソル直後の文字、無ければ直前の文字を起点候補にする。
    let cur_text = buffer.lines[cursor.line].text();
    let (start_line, start_byte, open_ch, close_ch, forward) = {
        // 直後 (cursor.index から始まる) の括弧を優先。
        let after = cur_text[cursor.index..].chars().next();
        if let Some((o, c, fwd)) = after.and_then(bracket_kind) {
            (cursor.line, cursor.index, o, c, fwd)
        } else {
            // 直前の 1 文字。
            let before_byte = cur_text[..cursor.index]
                .char_indices()
                .next_back()
                .map(|(i, _)| i);
            match before_byte.and_then(|b| {
                cur_text[b..]
                    .chars()
                    .next()
                    .and_then(bracket_kind)
                    .map(|(o, c, fwd)| (b, o, c, fwd))
            }) {
                Some((b, o, c, fwd)) => (cursor.line, b, o, c, fwd),
                None => return None,
            }
        }
    };

    // 起点の括弧の (line, byte_range)。
    let start_ch_len = open_at(buffer, start_line, start_byte)?;
    let start_span = (start_line, start_byte..start_byte + start_ch_len);

    let mut depth: i32 = 0;
    let mut scanned = 0usize;

    if forward {
        // open → close を前方スキャン。
        let mut li = start_line;
        let mut bi = start_byte;
        while li < line_count {
            let text = buffer.lines[li].text();
            let mut iter = text[bi..].char_indices();
            while let Some((rel, ch)) = iter.next() {
                let abs = bi + rel;
                if ch == open_ch {
                    depth += 1;
                } else if ch == close_ch {
                    depth -= 1;
                    if depth == 0 {
                        return Some([start_span, (li, abs..abs + ch.len_utf8())]);
                    }
                }
                scanned += 1;
                if scanned > SCAN_CAP {
                    return None;
                }
            }
            li += 1;
            bi = 0;
        }
    } else {
        // close → open を後方スキャン。
        let mut li = start_line as isize;
        let mut bi_end = start_byte + start_ch_len; // exclusive 上端
        while li >= 0 {
            let text = buffer.lines[li as usize].text();
            let slice = &text[..bi_end.min(text.len())];
            for (abs, ch) in slice.char_indices().rev() {
                if ch == close_ch {
                    depth += 1;
                } else if ch == open_ch {
                    depth -= 1;
                    if depth == 0 {
                        let open_span = (li as usize, abs..abs + ch.len_utf8());
                        // open, close の順で返す。
                        return Some([open_span, start_span]);
                    }
                }
                scanned += 1;
                if scanned > SCAN_CAP {
                    return None;
                }
            }
            li -= 1;
            if li >= 0 {
                bi_end = buffer.lines[li as usize].text().len();
            }
        }
    }

    None
}

/// 文字が括弧なら (open_char, close_char, is_open_so_scan_forward) を返す。
fn bracket_kind(ch: char) -> Option<(char, char, bool)> {
    match ch {
        '(' => Some(('(', ')', true)),
        '[' => Some(('[', ']', true)),
        '{' => Some(('{', '}', true)),
        ')' => Some(('(', ')', false)),
        ']' => Some(('[', ']', false)),
        '}' => Some(('{', '}', false)),
        _ => None,
    }
}

/// (line, byte) 位置の文字の UTF-8 バイト長を返す (範囲外なら None)。
fn open_at(buffer: &cosmic_text::Buffer, line: usize, byte: usize) -> Option<usize> {
    let text = buffer.lines.get(line)?.text();
    text[byte..].chars().next().map(|c| c.len_utf8())
}
