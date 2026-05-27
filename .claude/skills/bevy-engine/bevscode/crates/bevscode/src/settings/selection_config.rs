//! Selection + multi-cursor + occurrences-highlight settings — Monaco
//! `multiCursor*`, `selectionHighlight*`, `occurrencesHighlight*`,
//! `wordSeparators`, `columnSelection`, `doubleClickSelectsBlock`,
//! `copyWithSyntaxHighlighting`, `emptySelectionClipboard`, `roundedSelection`.
//!
//! Named `SelectionConfig` because `bevscode::types::Selection` is already a
//! re-exported span type from the interaction crate.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct SelectionConfig {
    pub multi_cursor_modifier: MultiCursorModifier,
    pub merge_overlapping: bool,
    pub paste: MultiCursorPaste,
    pub limit: u32,
    pub column_selection: bool,
    pub selection_highlight: bool,
    pub selection_highlight_multiline: bool,
    pub selection_highlight_max_length: u32,
    pub occurrences_highlight: OccurrencesHighlight,
    pub occurrences_highlight_delay_ms: u32,
    pub word_separators: String,
    pub empty_selection_clipboard: bool,
    pub copy_with_syntax_highlighting: bool,
    pub double_click_selects_block: bool,
    pub rounded_selection: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MultiCursorModifier {
    CtrlCmd,
    #[default]
    Alt,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MultiCursorPaste {
    #[default]
    Spread,
    Full,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum OccurrencesHighlight {
    Off,
    #[default]
    SingleFile,
    MultiFile,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            multi_cursor_modifier: MultiCursorModifier::Alt,
            merge_overlapping: true,
            paste: MultiCursorPaste::Spread,
            limit: 10_000,
            column_selection: false,
            selection_highlight: true,
            selection_highlight_multiline: false,
            selection_highlight_max_length: 200,
            occurrences_highlight: OccurrencesHighlight::SingleFile,
            occurrences_highlight_delay_ms: 250,
            word_separators: String::from("`~!@#$%^&*()-=+[{]}\\|;:'\",.<>/?"),
            empty_selection_clipboard: true,
            copy_with_syntax_highlighting: true,
            double_click_selects_block: true,
            rounded_selection: true,
        }
    }
}
