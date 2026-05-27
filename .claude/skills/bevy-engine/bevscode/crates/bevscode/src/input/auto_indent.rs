//! Enter-key auto-indent + dedent-on-close-brace logic.
//!
//! Pure functions over [`ropey::Rope`] and the [`crate::settings`] structs;
//! no Bevy types so the rules stay unit-testable and language-agnostic for
//! the modes that don't require a syntax tree.

use crate::settings::{AutoEdit, AutoIndent, Indentation};
use ropey::Rope;

/// Compute the indent string that follows the inserted `\n` for an Enter
/// keystroke at `cursor_pos`. Caller emits
/// `ReplaceRangeRequested { text: format!("\n{indent}"), .. }`.
///
/// `mode` is expected to be non-[`AutoIndent::None`]; callers gate that path
/// separately and emit the plain `InsertNewlineRequested` instead.
pub fn compute_newline_indent(
    rope: &Rope,
    cursor_pos: usize,
    mode: AutoIndent,
    indent: &Indentation,
) -> String {
    let cursor_pos = cursor_pos.min(rope.len_chars());
    let mut result = leading_whitespace_of_line(rope, cursor_pos);

    if matches!(
        mode,
        AutoIndent::Brackets | AutoIndent::Advanced | AutoIndent::Full
    ) && last_non_ws_before(rope, cursor_pos).is_some_and(|c| matches!(c, '{' | '(' | '['))
    {
        result.push_str(&one_indent_level(indent));
    }

    result
}

/// If typing `c` on the current line should dedent before insertion, returns
/// the number of leading whitespace chars to remove. Returns `None` when the
/// line is not whitespace-only up to the cursor, when no indent level is
/// present, or when the mode doesn't enable bracket-aware indent.
pub fn should_dedent_close_brace(
    c: char,
    rope: &Rope,
    cursor_pos: usize,
    auto_edit: &AutoEdit,
    indent: &Indentation,
) -> Option<usize> {
    if !matches!(c, '}' | ')' | ']') {
        return None;
    }
    if !matches!(
        auto_edit.auto_indent,
        AutoIndent::Brackets | AutoIndent::Advanced | AutoIndent::Full
    ) {
        return None;
    }

    let cursor_pos = cursor_pos.min(rope.len_chars());
    let line_idx = rope.char_to_line(cursor_pos);
    let line_start = rope.line_to_char(line_idx);
    let prefix_len = cursor_pos - line_start;

    let mut leading = 0usize;
    for (i, ch) in rope.line(line_idx).chars().take(prefix_len).enumerate() {
        if ch == ' ' || ch == '\t' {
            leading = i + 1;
        } else {
            return None;
        }
    }
    if leading != prefix_len {
        return None;
    }

    let width = indent_width(indent);
    if width == 0 || leading < width {
        return None;
    }
    Some(width)
}

fn leading_whitespace_of_line(rope: &Rope, cursor_pos: usize) -> String {
    let line_idx = rope.char_to_line(cursor_pos);
    let line_start = rope.line_to_char(line_idx);
    let prefix_len = cursor_pos - line_start;
    let mut buf = String::new();
    for ch in rope.line(line_idx).chars().take(prefix_len) {
        if ch == ' ' || ch == '\t' {
            buf.push(ch);
        } else {
            break;
        }
    }
    buf
}

fn last_non_ws_before(rope: &Rope, cursor_pos: usize) -> Option<char> {
    let line_idx = rope.char_to_line(cursor_pos);
    let line_start = rope.line_to_char(line_idx);
    let prefix_len = cursor_pos - line_start;
    let mut last = None;
    for ch in rope.line(line_idx).chars().take(prefix_len) {
        if !ch.is_whitespace() {
            last = Some(ch);
        }
    }
    last
}

fn one_indent_level(indent: &Indentation) -> String {
    if indent.insert_spaces {
        " ".repeat(indent_width(indent))
    } else {
        "\t".to_string()
    }
}

fn indent_width(indent: &Indentation) -> usize {
    indent.indent_size.resolve(indent.tab_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{AutoEdit, AutoIndent, IndentSize, Indentation};

    fn spaces_4() -> Indentation {
        Indentation {
            tab_size: 4,
            insert_spaces: true,
            indent_size: IndentSize::TabSize,
            ..Default::default()
        }
    }

    fn tabs() -> Indentation {
        Indentation {
            tab_size: 4,
            insert_spaces: false,
            indent_size: IndentSize::TabSize,
            ..Default::default()
        }
    }

    #[test]
    fn keep_copies_leading_whitespace() {
        let rope = Rope::from_str("    let x = 1;");
        let pos = rope.len_chars();
        let s = compute_newline_indent(&rope, pos, AutoIndent::Keep, &spaces_4());
        assert_eq!(s, "    ");
    }

    #[test]
    fn keep_handles_mid_line_split() {
        let rope = Rope::from_str("    foobar");
        let pos = 7;
        let s = compute_newline_indent(&rope, pos, AutoIndent::Keep, &spaces_4());
        assert_eq!(s, "    ");
    }

    #[test]
    fn keep_empty_line_yields_empty_indent() {
        let rope = Rope::from_str("");
        let s = compute_newline_indent(&rope, 0, AutoIndent::Keep, &spaces_4());
        assert_eq!(s, "");
    }

    #[test]
    fn brackets_adds_one_level_after_open_brace() {
        let rope = Rope::from_str("fn main() {");
        let pos = rope.len_chars();
        let s = compute_newline_indent(&rope, pos, AutoIndent::Brackets, &spaces_4());
        assert_eq!(s, "    ");
    }

    #[test]
    fn brackets_skips_when_no_open_brace() {
        let rope = Rope::from_str("    let x = 1;");
        let pos = rope.len_chars();
        let s = compute_newline_indent(&rope, pos, AutoIndent::Brackets, &spaces_4());
        assert_eq!(s, "    ");
    }

    #[test]
    fn tab_indented_input_yields_tab_indent_string() {
        let rope = Rope::from_str("\tfn inner() {");
        let pos = rope.len_chars();
        let s = compute_newline_indent(&rope, pos, AutoIndent::Brackets, &tabs());
        assert_eq!(s, "\t\t");
    }

    fn ae_with(mode: AutoIndent) -> AutoEdit {
        AutoEdit {
            auto_indent: mode,
            ..Default::default()
        }
    }

    #[test]
    fn dedent_close_brace_on_indent_only_line() {
        let rope = Rope::from_str("    ");
        let ae = ae_with(AutoIndent::Brackets);
        let got = should_dedent_close_brace('}', &rope, rope.len_chars(), &ae, &spaces_4());
        assert_eq!(got, Some(4));
    }

    #[test]
    fn dedent_close_brace_skipped_mid_line() {
        let rope = Rope::from_str("    code");
        let ae = ae_with(AutoIndent::Brackets);
        let got = should_dedent_close_brace('}', &rope, rope.len_chars(), &ae, &spaces_4());
        assert_eq!(got, None);
    }

    #[test]
    fn dedent_close_brace_skipped_when_auto_indent_none() {
        let rope = Rope::from_str("    ");
        let ae = ae_with(AutoIndent::None);
        let got = should_dedent_close_brace('}', &rope, rope.len_chars(), &ae, &spaces_4());
        assert_eq!(got, None);
    }
}
