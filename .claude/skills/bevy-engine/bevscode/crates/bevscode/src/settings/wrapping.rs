//! Word-wrap settings — Monaco `wordWrap`, `wordWrapColumn`, `wrappingIndent`,
//! `wrappingStrategy`, `wordBreak`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Wrapping {
    pub word_wrap: WordWrapMode,
    pub word_wrap_column: u32,
    pub wrapping_indent: WrappingIndent,
    pub wrapping_strategy: WrappingStrategy,
    pub word_break: WordBreak,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum WordWrapMode {
    #[default]
    Off,
    On,
    WordWrapColumn,
    Bounded,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum WrappingIndent {
    None,
    #[default]
    Same,
    Indent,
    DeepIndent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum WrappingStrategy {
    #[default]
    Simple,
    Advanced,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum WordBreak {
    #[default]
    Normal,
    KeepAll,
}

impl Default for Wrapping {
    fn default() -> Self {
        Self {
            word_wrap: WordWrapMode::Off,
            word_wrap_column: 80,
            wrapping_indent: WrappingIndent::Same,
            wrapping_strategy: WrappingStrategy::Simple,
            word_break: WordBreak::Normal,
        }
    }
}
