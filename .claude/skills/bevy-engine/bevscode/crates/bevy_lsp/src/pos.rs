//! Rope ↔ LSP `Position` conversion. The protocol's `character` field is
//! measured in code units of a negotiated [`PositionEncoding`]; UTF-16 is the
//! spec default, so callers that don't negotiate should pass that.

use lsp_types::{Position, Range};
use ropey::Rope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
    Utf32,
}

impl PositionEncoding {
    fn units_in(self, c: char) -> usize {
        match self {
            Self::Utf8 => c.len_utf8(),
            Self::Utf16 => c.len_utf16(),
            Self::Utf32 => 1,
        }
    }
}

pub fn rope_char_to_lsp_position(
    rope: &Rope,
    char_offset: usize,
    encoding: PositionEncoding,
) -> Position {
    let char_offset = char_offset.min(rope.len_chars());
    let line = rope.char_to_line(char_offset);
    let line_start_char = rope.line_to_char(line);
    let character = count_units(rope, line_start_char, char_offset, encoding);
    Position {
        line: line as u32,
        character: character as u32,
    }
}

pub fn rope_byte_to_lsp_position(
    rope: &Rope,
    byte_offset: usize,
    encoding: PositionEncoding,
) -> Position {
    let byte_offset = byte_offset.min(rope.len_bytes());
    rope_char_to_lsp_position(rope, rope.byte_to_char(byte_offset), encoding)
}

pub fn rope_range_to_lsp_range(
    rope: &Rope,
    start_char: usize,
    end_char: usize,
    encoding: PositionEncoding,
) -> Range {
    Range {
        start: rope_char_to_lsp_position(rope, start_char, encoding),
        end: rope_char_to_lsp_position(rope, end_char, encoding),
    }
}

pub fn lsp_position_to_rope_char(
    rope: &Rope,
    position: Position,
    encoding: PositionEncoding,
) -> usize {
    let line = (position.line as usize).min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_char(line);
    let line_chars = rope.line(line).len_chars();
    let target_units = position.character as usize;

    let mut units = 0usize;
    let line_slice = rope.line(line);
    for (i, c) in line_slice.chars().enumerate() {
        if units >= target_units {
            return line_start + i;
        }
        units += encoding.units_in(c);
    }
    line_start + line_chars
}

pub fn lsp_position_to_rope_byte(
    rope: &Rope,
    position: Position,
    encoding: PositionEncoding,
) -> usize {
    let char_offset = lsp_position_to_rope_char(rope, position, encoding);
    rope.char_to_byte(char_offset)
}

fn count_units(
    rope: &Rope,
    start_char: usize,
    end_char: usize,
    encoding: PositionEncoding,
) -> usize {
    if start_char == end_char {
        return 0;
    }
    let slice = rope.slice(start_char..end_char);
    match encoding {
        PositionEncoding::Utf8 => slice.len_bytes(),
        PositionEncoding::Utf16 => slice.chars().map(|c| c.len_utf16()).sum(),
        PositionEncoding::Utf32 => slice.len_chars(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        name: &'static str,
        text: &'static str,
        char_offset: usize,
        line: u32,
        utf8: u32,
        utf16: u32,
        utf32: u32,
    }

    const CASES: &[Case] = &[
        Case {
            name: "ascii start of line",
            text: "hello\nworld",
            char_offset: 0,
            line: 0,
            utf8: 0,
            utf16: 0,
            utf32: 0,
        },
        Case {
            name: "ascii mid line",
            text: "hello\nworld",
            char_offset: 3,
            line: 0,
            utf8: 3,
            utf16: 3,
            utf32: 3,
        },
        Case {
            name: "ascii on second line",
            text: "hello\nworld",
            char_offset: 8,
            line: 1,
            utf8: 2,
            utf16: 2,
            utf32: 2,
        },
        Case {
            // 'é' is 1 char, 2 utf-8 bytes, 1 utf-16 code unit.
            name: "latin-1 supplemental (é)",
            text: "café\n",
            char_offset: 4, // after 'é'
            line: 0,
            utf8: 5,
            utf16: 4,
            utf32: 4,
        },
        Case {
            // BMP CJK '中' is 1 char, 3 utf-8 bytes, 1 utf-16 code unit.
            name: "BMP CJK (中)",
            text: "中文\n",
            char_offset: 2,
            line: 0,
            utf8: 6,
            utf16: 2,
            utf32: 2,
        },
        Case {
            // '🎉' (U+1F389) is 1 char, 4 utf-8 bytes, 2 utf-16 code units (surrogate pair).
            name: "supplementary plane (🎉)",
            text: "a🎉b\n",
            char_offset: 2, // after the emoji
            line: 0,
            utf8: 5,
            utf16: 3,
            utf32: 2,
        },
        Case {
            // Same emoji, on a continuation line, after some ASCII.
            name: "emoji on second line",
            text: "ok\nx🎉y",
            char_offset: 5, // 'ok\n' = 3 chars; 'x🎉' = 2 chars
            line: 1,
            utf8: 5,  // 'x' (1) + '🎉' (4)
            utf16: 3, // 'x' (1) + '🎉' (2)
            utf32: 2,
        },
    ];

    fn each(enc: PositionEncoding, char: impl Fn(&Case) -> u32) {
        for c in CASES {
            let rope = Rope::from_str(c.text);
            let pos = rope_char_to_lsp_position(&rope, c.char_offset, enc);
            assert_eq!(
                pos,
                Position {
                    line: c.line,
                    character: char(c)
                },
                "case {} ({:?}) char_offset={}",
                c.name,
                enc,
                c.char_offset
            );

            // Round-trip back to char offset.
            let back = lsp_position_to_rope_char(&rope, pos, enc);
            assert_eq!(
                back,
                c.char_offset.min(rope.len_chars()),
                "round-trip case {} ({:?})",
                c.name,
                enc
            );
        }
    }

    #[test]
    fn rope_to_lsp_utf8() {
        each(PositionEncoding::Utf8, |c| c.utf8);
    }

    #[test]
    fn rope_to_lsp_utf16() {
        each(PositionEncoding::Utf16, |c| c.utf16);
    }

    #[test]
    fn rope_to_lsp_utf32() {
        each(PositionEncoding::Utf32, |c| c.utf32);
    }

    #[test]
    fn clamps_past_end_of_line() {
        let rope = Rope::from_str("ab\ncd");
        // Asking for column 99 on line 0 should land at end of "ab".
        let pos = Position {
            line: 0,
            character: 99,
        };
        let off = lsp_position_to_rope_char(&rope, pos, PositionEncoding::Utf16);
        // "ab\n" = 3 chars; rope.line(0).len_chars() includes the newline = 3.
        assert_eq!(off, 3);
    }

    #[test]
    fn clamps_past_end_of_rope() {
        let rope = Rope::from_str("ab");
        let pos = Position {
            line: 99,
            character: 0,
        };
        let off = lsp_position_to_rope_char(&rope, pos, PositionEncoding::Utf16);
        // Falls back to last line start.
        assert_eq!(off, 0);
    }

    #[test]
    fn byte_round_trip_through_lsp() {
        let rope = Rope::from_str("hi 🎉 there");
        for byte in [0usize, 1, 2, 3, 7, 8, 13] {
            // Skip non-char-boundary bytes (the emoji's interior).
            if rope.try_byte_to_char(byte).is_err() {
                continue;
            }
            let pos = rope_byte_to_lsp_position(&rope, byte, PositionEncoding::Utf16);
            let back = lsp_position_to_rope_byte(&rope, pos, PositionEncoding::Utf16);
            assert_eq!(back, byte, "byte {} round-trip", byte);
        }
    }

    #[test]
    fn range_construction() {
        let rope = Rope::from_str("a🎉b\ncd");
        let range = rope_range_to_lsp_range(&rope, 1, 2, PositionEncoding::Utf16);
        // Char 1 = before emoji = column 1 (utf16: 1 unit for 'a').
        // Char 2 = after emoji = column 3 (utf16: 1 + 2 surrogates).
        assert_eq!(
            range.start,
            Position {
                line: 0,
                character: 1
            }
        );
        assert_eq!(
            range.end,
            Position {
                line: 0,
                character: 3
            }
        );
    }
}
