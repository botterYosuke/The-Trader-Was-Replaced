//! IntelliSense / suggestions / hover / inlay hints — Monaco
//! `quickSuggestions*`, `suggestOnTriggerCharacters`, `acceptSuggestionOn*`,
//! `snippetSuggestions`, `tabCompletion`, `wordBasedSuggestions`,
//! `parameterHints`, `hover`, `inlayHints`, `inlineSuggest`, `codeLens`.
//!
//! Cfg-gated on the `lsp` feature.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Suggest {
    pub quick_suggestions: QuickSuggestions,
    pub quick_suggestions_delay_ms: u32,
    pub on_trigger_characters: bool,
    pub accept_on_enter: AcceptSuggestionOnEnter,
    pub accept_on_commit_character: bool,
    pub snippet_suggestions: SnippetSuggestions,
    pub tab_completion: TabCompletion,
    pub selection_mode: SuggestSelection,
    pub word_based_suggestions: WordBasedSuggestions,
    pub parameter_hints: ParameterHints,
    pub hover: Hover,
    pub inlay_hints: InlayHints,
    pub inline_suggest: InlineSuggest,
    pub code_lens: bool,
    pub show_unused: bool,
    pub show_deprecated: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct QuickSuggestions {
    pub other: QuickSuggestion,
    pub comments: QuickSuggestion,
    pub strings: QuickSuggestion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum QuickSuggestion {
    #[default]
    On,
    Off,
    Inline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AcceptSuggestionOnEnter {
    #[default]
    On,
    Smart,
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum SnippetSuggestions {
    Top,
    Bottom,
    #[default]
    Inline,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum TabCompletion {
    On,
    #[default]
    Off,
    OnlySnippets,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum SuggestSelection {
    #[default]
    First,
    RecentlyUsed,
    RecentlyUsedByPrefix,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum WordBasedSuggestions {
    Off,
    CurrentDocument,
    #[default]
    MatchingDocuments,
    AllDocuments,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct ParameterHints {
    pub enabled: bool,
    pub cycle: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct Hover {
    pub enabled: bool,
    pub delay_ms: u32,
    pub hiding_delay_ms: u32,
    pub sticky: bool,
    pub above: bool,
    pub show_long_line_warning: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct InlayHints {
    pub enabled: InlayHintsEnabled,
    pub font_size: u32,
    pub padding: bool,
    pub maximum_length: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum InlayHintsEnabled {
    #[default]
    On,
    OnUnlessPressed,
    OffUnlessPressed,
    Off,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct InlineSuggest {
    pub enabled: bool,
    pub show_toolbar: InlineSuggestShowToolbar,
    pub suppress_suggestions: bool,
    pub keep_on_blur: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum InlineSuggestShowToolbar {
    Always,
    #[default]
    OnHover,
    Never,
}

impl Default for QuickSuggestions {
    fn default() -> Self {
        Self {
            other: QuickSuggestion::On,
            comments: QuickSuggestion::Off,
            strings: QuickSuggestion::Off,
        }
    }
}

impl Default for ParameterHints {
    fn default() -> Self {
        Self {
            enabled: true,
            cycle: true,
        }
    }
}

impl Default for Hover {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_ms: 300,
            hiding_delay_ms: 300,
            sticky: true,
            above: true,
            show_long_line_warning: true,
        }
    }
}

impl Default for InlayHints {
    fn default() -> Self {
        Self {
            enabled: InlayHintsEnabled::On,
            font_size: 0,
            padding: false,
            maximum_length: 43,
        }
    }
}

impl Default for InlineSuggest {
    fn default() -> Self {
        Self {
            enabled: true,
            show_toolbar: InlineSuggestShowToolbar::OnHover,
            suppress_suggestions: false,
            keep_on_blur: false,
        }
    }
}

impl Default for Suggest {
    fn default() -> Self {
        Self {
            quick_suggestions: QuickSuggestions::default(),
            quick_suggestions_delay_ms: 10,
            on_trigger_characters: true,
            accept_on_enter: AcceptSuggestionOnEnter::On,
            accept_on_commit_character: true,
            snippet_suggestions: SnippetSuggestions::Inline,
            tab_completion: TabCompletion::Off,
            selection_mode: SuggestSelection::First,
            word_based_suggestions: WordBasedSuggestions::MatchingDocuments,
            parameter_hints: ParameterHints::default(),
            hover: Hover::default(),
            inlay_hints: InlayHints::default(),
            inline_suggest: InlineSuggest::default(),
            code_lens: true,
            show_unused: true,
            show_deprecated: true,
        }
    }
}
