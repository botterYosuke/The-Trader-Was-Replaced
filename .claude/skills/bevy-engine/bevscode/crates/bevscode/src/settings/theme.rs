//! Per-entity editor theme.
//!
//! Theme is a per-entity Component, not a global Resource: multi-editor
//! apps can run a dark editor next to a light one without splitting state.
//! Bevy's `#[require]` cascade attaches `EditorTheme::default()` to every
//! `CodeEditor` entity, so spawning works without specifying colors.
//!
//! For different colors, override at spawn time
//! (`(CodeEditor, EditorTheme { background: ..., ..default() })`) or
//! mutate the Component at runtime via `Query<&mut EditorTheme, With<CodeEditor>>`.
//!
//! Syntax-coloring lives on a sibling `SyntaxColors` Component (cfg
//! `tree-sitter`); LSP diagnostic colors on `DiagnosticColors` (cfg `lsp`).
//! Both are also `#[require]`d by `CodeEditor` so they default in.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Per-entity editor color palette.
#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct EditorTheme {
    pub background: Color,
    pub foreground: Color,
    pub cursor: Color,
    pub selection_background: Color,
    /// `None` disables the current-line highlight band.
    pub line_highlight: Option<Color>,
    pub line_numbers: Color,
    pub line_numbers_active: Color,
    pub separator: Color,
    pub indent_guide: Color,
    pub bracket_match: Color,
    /// Rotating palette for bracket-pair colorization.
    pub bracket_pair_colors: Vec<Color>,
    pub placeholder_color: Color,
    /// Row-background tint drawn across the visible line of a folded region
    /// when `Folding::highlight` is true. Mirrors VSCode's
    /// `editor.foldBackground` — a subtle selection-like wash, not an
    /// underline.
    pub fold_marker: Color,
    /// Color used for whitespace markers (centered dot for spaces, thin
    /// bar for tabs) when `RenderSettings::render_whitespace` enables
    /// any visible mode. Mirrors VSCode's `editorWhitespace.foreground` —
    /// a low-alpha foreground tone so markers stay subtle.
    pub whitespace: Color,
    /// Color used to underline detected URLs when `Misc::links` is on.
    /// Mirrors VSCode's `textLink.foreground` — a muted blue that reads
    /// as a link without competing with syntax colors.
    pub link: Color,
}

impl Default for EditorTheme {
    fn default() -> Self {
        let palette = tempera::theme::ColorPalette::dark();
        Self {
            background: palette.background,
            foreground: palette.foreground,
            cursor: Color::srgb(0.682, 0.686, 0.678),
            selection_background: Color::srgba(0.149, 0.310, 0.471, 1.0),
            line_highlight: Some(Color::srgba(0.157, 0.157, 0.157, 0.6)),
            line_numbers: Color::srgb(0.431, 0.471, 0.506),
            line_numbers_active: Color::srgb(0.800, 0.800, 0.800),
            separator: palette.border,
            indent_guide: Color::srgba(0.251, 0.251, 0.251, 1.0),
            bracket_match: Color::srgba(0.0, 1.0, 0.5, 0.3),
            bracket_pair_colors: vec![
                Color::srgb(0.86, 0.86, 0.26),
                Color::srgb(0.85, 0.42, 0.85),
                Color::srgb(0.20, 0.74, 0.91),
                Color::srgb(0.96, 0.55, 0.24),
                Color::srgb(0.40, 0.83, 0.40),
                Color::srgb(0.93, 0.36, 0.39),
            ],
            placeholder_color: palette.muted_foreground,
            fold_marker: Color::srgba(0.231, 0.373, 0.604, 0.18),
            whitespace: Color::srgba(0.5, 0.5, 0.5, 0.18),
            link: Color::srgba(0.29, 0.56, 0.89, 0.6),
        }
    }
}

/// Per-entity LSP diagnostic colors. Cfg-gated on the `lsp` feature.
#[cfg(feature = "lsp")]
#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct DiagnosticColors {
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub hint: Color,
}

#[cfg(feature = "lsp")]
impl Default for DiagnosticColors {
    fn default() -> Self {
        let tokens = crate::ui_kit::DiagnosticTokens::default();
        Self {
            error: tokens.error,
            warning: tokens.warning,
            info: tokens.info,
            hint: tokens.hint,
        }
    }
}
