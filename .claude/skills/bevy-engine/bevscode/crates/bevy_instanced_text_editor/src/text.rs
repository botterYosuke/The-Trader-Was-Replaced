//! Rope-backed [`TextContent`] implementation for editable text views.

use std::borrow::Cow;
use std::ops::{Deref, DerefMut, Range};

use bevy::prelude::*;
use bevy_instanced_text::TextContent;
use ropey::Rope;

#[derive(Component, Clone, Default)]
pub struct RopeBuffer(pub Rope);

impl RopeBuffer {
    pub fn new(text: &str) -> Self {
        Self(Rope::from_str(text))
    }

    pub fn rope(&self) -> &Rope {
        &self.0
    }
}

impl Deref for RopeBuffer {
    type Target = Rope;
    fn deref(&self) -> &Rope {
        &self.0
    }
}

impl DerefMut for RopeBuffer {
    fn deref_mut(&mut self) -> &mut Rope {
        &mut self.0
    }
}

impl TextContent for RopeBuffer {
    fn line_count(&self) -> usize {
        self.0.len_lines()
    }

    fn line(&self, i: usize) -> Cow<'_, str> {
        if i >= self.0.len_lines() {
            return Cow::Borrowed("");
        }
        Cow::Owned(self.0.line(i).to_string())
    }

    fn line_len_chars(&self, i: usize) -> usize {
        if i >= self.0.len_lines() {
            return 0;
        }
        let l = self.0.line(i);
        let mut n = l.len_chars();
        if n > 0 && l.char(n - 1) == '\n' {
            n -= 1;
        }
        n
    }

    // Override the default O(n) walkers with rope-backed O(log n) versions.
    fn char_count(&self) -> usize {
        self.0.len_chars()
    }

    fn line_to_char(&self, line: usize) -> usize {
        let clamped = line.min(self.0.len_lines());
        self.0.line_to_char(clamped)
    }

    fn char_to_line(&self, ch: usize) -> usize {
        let clamped = ch.min(self.0.len_chars());
        self.0.char_to_line(clamped)
    }

    fn slice_chars(&self, range: Range<usize>) -> Cow<'_, str> {
        let total = self.0.len_chars();
        let start = range.start.min(total);
        let end = range.end.min(total).max(start);
        Cow::Owned(self.0.slice(start..end).to_string())
    }
}
