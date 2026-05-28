//! LSP (Language Server Protocol) settings

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-editor LSP settings: debounce timers, completion/hover UI behavior.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct LspConfig {
    /// Auto-completion settings
    pub completion: CompletionConfig,

    /// Hover information settings
    pub hover: HoverConfig,

    /// Debounce before requesting `textDocument/documentHighlight` after
    /// the cursor stops moving. Wired into `request_document_highlights`.
    pub highlight_delay_ms: u64,

    /// Debounce before flushing `textDocument/didChange` after the buffer
    /// is edited. Lower values mean faster diagnostics; higher values mean
    /// fewer wasted server-side reparses.
    pub did_change_delay_ms: u64,

    /// Force full-document `textDocument/didChange` payloads instead of
    /// incremental ones. Useful as a recovery flag if a position-encoding
    /// bug ever surfaces in incremental sync. Default `false` (incremental).
    pub full_document_sync: bool,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            completion: CompletionConfig::default(),
            hover: HoverConfig::default(),
            highlight_delay_ms: 100,
            did_change_delay_ms: 150,
            full_document_sync: false,
        }
    }
}

/// How buffer-words feed the completion popup. Mirrors Zed's
/// `WordsCompletionMode`: words can run alongside LSP results, only
/// fill in when LSP is silent, or be turned off entirely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Default, Debug)]
pub enum WordsCompletionMode {
    /// Always merge buffer words with LSP results.
    Always,
    /// Only show buffer words when LSP returned no results or marked the
    /// list as incomplete. This is the default.
    #[default]
    Fallback,
    /// Never show buffer words.
    Disabled,
}

/// Auto-completion settings
#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Default, Debug)]
pub struct CompletionConfig {
    /// Enable auto-completion
    pub enabled: bool,

    /// Trigger characters that auto-open completion
    pub trigger_characters: Vec<String>,

    /// Minimum word length before auto-triggering completion
    pub min_word_length: usize,

    /// Delay before showing completion (milliseconds)
    pub delay_ms: u64,

    /// Maximum number of completion items to show
    pub max_items: usize,

    /// Completion window width (pixels)
    pub window_width: f32,

    /// Completion window background color
    pub window_background: Color,

    /// Selected item background color
    pub selected_background: Color,

    /// Completion text color
    pub text_color: Color,

    /// Selected item text color
    pub selected_text_color: Color,

    /// How to merge buffer-word completions with LSP results.
    pub words_mode: WordsCompletionMode,
}

/// Hover information settings
#[derive(Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Default, Debug)]
pub struct HoverConfig {
    /// Enable hover information
    pub enabled: bool,

    /// Delay before showing hover (milliseconds)
    pub delay_ms: u64,

    /// Dismiss-grace window after the pointer leaves the editor or the
    /// hover hot zone. The popup stays visible for this long so the
    /// user can move the cursor onto the popup chrome (e.g. to scroll
    /// long docs) without the popup vanishing under them.
    pub hiding_delay_ms: u32,

    /// Hover window max width (pixels)
    pub max_width: f32,

    /// Hover window background color
    pub background_color: Color,

    /// Hover text color
    pub text_color: Color,

    /// Hover border color
    pub border_color: Color,

    /// Hover border width (pixels)
    pub border_width: f32,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_characters: vec![".".to_string(), "::".to_string()],
            min_word_length: 3,
            delay_ms: 100,
            max_items: 10,
            window_width: 300.0,
            window_background: Color::srgba(0.15, 0.15, 0.15, 0.95),
            selected_background: Color::srgb(0.25, 0.35, 0.5),
            text_color: Color::srgb(0.85, 0.85, 0.85),
            selected_text_color: Color::srgb(1.0, 1.0, 1.0),
            words_mode: WordsCompletionMode::default(),
        }
    }
}

impl Default for HoverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_ms: 300,
            hiding_delay_ms: 300,
            max_width: 500.0,
            background_color: Color::srgba(0.15, 0.15, 0.15, 0.95),
            text_color: Color::srgb(0.85, 0.85, 0.85),
            border_color: Color::srgb(0.3, 0.3, 0.3),
            border_width: 1.0,
        }
    }
}
