//! Word-boundary helpers parametrized by `SelectionConfig::word_separators`.
//!
//! Centralizes the "is this char part of a word" predicate plus
//! `find_word_start`/`find_word_end` so cursor word-left/right, double-click
//! selection, word-highlight, and `DeleteWord*` all derive their boundary
//! from the same source.

use ropey::Rope;

/// `true` when `c` should count as part of a word given a Monaco-style
/// separator string. A char belongs to a word iff it is not whitespace
/// and not present in `separators`.
pub fn is_word_char(c: char, separators: &str) -> bool {
    if c.is_whitespace() {
        return false;
    }
    !separators.contains(c)
}

/// Walk left from `pos` to the start of the word currently under or just
/// before the cursor. Returns `pos` when not inside a word.
pub fn find_word_start(rope: &Rope, pos: usize, separators: &str) -> usize {
    let mut current = pos;
    while current > 0 {
        let prev = rope.char(current - 1);
        if is_word_char(prev, separators) {
            current -= 1;
        } else {
            break;
        }
    }
    current
}

/// Walk right from `pos` to the end of the word currently under the cursor.
/// Returns `pos` when not inside a word.
pub fn find_word_end(rope: &Rope, pos: usize, separators: &str) -> usize {
    let len = rope.len_chars();
    let mut current = pos;
    while current < len {
        let c = rope.char(current);
        if is_word_char(c, separators) {
            current += 1;
        } else {
            break;
        }
    }
    current
}
