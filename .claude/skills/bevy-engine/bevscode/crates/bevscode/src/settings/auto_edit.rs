//! Auto-edit behavior — Monaco `autoClosing*`, `autoSurround`, `autoIndent`,
//! `formatOn*`, `linkedEditing`, `trimAutoWhitespace`, plus the bracket pair list.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct AutoEdit {
    pub auto_closing_brackets: AutoClosingPairs,
    pub auto_closing_quotes: AutoClosingPairs,
    pub auto_closing_comments: AutoClosingPairs,
    pub auto_closing_delete: AutoClosingAuto,
    pub auto_closing_overtype: AutoClosingAuto,
    pub auto_surround: AutoSurround,
    pub auto_indent: AutoIndent,
    pub auto_indent_on_paste: bool,
    pub format_on_type: bool,
    pub format_on_paste: bool,
    pub linked_editing: bool,
    pub trim_auto_whitespace: bool,
    pub pairs: Vec<(char, char)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AutoClosingPairs {
    Always,
    #[default]
    LanguageDefined,
    BeforeWhitespace,
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AutoClosingAuto {
    Always,
    #[default]
    Auto,
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AutoSurround {
    #[default]
    LanguageDefined,
    Quotes,
    Brackets,
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AutoIndent {
    None,
    Keep,
    Brackets,
    #[default]
    Advanced,
    Full,
}

impl Default for AutoEdit {
    fn default() -> Self {
        Self {
            auto_closing_brackets: AutoClosingPairs::LanguageDefined,
            auto_closing_quotes: AutoClosingPairs::LanguageDefined,
            auto_closing_comments: AutoClosingPairs::LanguageDefined,
            auto_closing_delete: AutoClosingAuto::Auto,
            auto_closing_overtype: AutoClosingAuto::Auto,
            auto_surround: AutoSurround::LanguageDefined,
            auto_indent: AutoIndent::Advanced,
            auto_indent_on_paste: true,
            format_on_type: false,
            format_on_paste: false,
            linked_editing: false,
            trim_auto_whitespace: true,
            pairs: vec![('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')],
        }
    }
}
