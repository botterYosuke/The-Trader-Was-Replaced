//! Gutter + line-number + indentation + bracket-match settings.
//!
//! Monaco parity:
//! - `EditorUi` → `lineNumbers`, `lineNumbersMinChars`, `glyphMargin`,
//!   `lineDecorationsWidth`, `selectOnLineNumbers`, `placeholder`.
//! - `Indentation` → `tabSize`, `insertSpaces`, `detectIndentation`,
//!   `indentSize`, `useTabStops`, `stickyTabStops`, `trimWhitespaceOnDelete`.
//! - `BracketConfig` → `matchBrackets`, `bracketPairColorization`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct EditorUi {
    pub line_numbers: LineNumbers,
    pub line_numbers_min_chars: u32,
    pub glyph_margin: bool,
    /// Width in pixels of the glyph-margin column when `glyph_margin` is
    /// true. Sized to fit a small dot / icon — Monaco's default is 16.
    pub glyph_margin_width: f32,
    /// Width of the line-decorations strip (VCS bars, severity bars).
    /// `0.0` disables the column.
    pub line_decorations_width: f32,
    pub select_on_line_numbers: bool,
    pub show_gutter: bool,
    pub show_separator: bool,
    pub gutter_padding_left: f32,
    pub gutter_padding_right: f32,
    pub code_margin_left: f32,
    pub placeholder: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum LineNumbers {
    #[default]
    On,
    Off,
    Relative,
    Interval,
}

/// Resolved gutter geometry. Single per-editor source of truth for
/// column widths and offsets. Populated by `resolve_gutter_layout`;
/// every other gutter system reads it and never recomputes offsets.
///
/// Column model (Monaco parity, left → right):
/// `[ pad_l | glyph | numbers | decorations(chevron|bar) | pad_r | code_margin ]`
///
/// Invariants:
/// - `gutter_width == sum(band widths) + ui.gutter_padding_left + ui.gutter_padding_right`
/// - For adjacent bands B1, B2: `B1.right() == B2.left`.
/// - `editor_padding_left == gutter_width + ui.code_margin_left` —
///   `Node::padding.left` on the editor is derived from this, so the
///   gutter container width and editor's left padding cannot drift.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Reflect)]
#[reflect(Component, Default, Debug, PartialEq)]
pub struct GutterConfig {
    pub gutter_width: f32,
    pub editor_padding_left: f32,
    pub line_height_px: f32,

    pub glyph: GutterBand,
    pub numbers: GutterBand,
    /// Outer band that contains both the fold chevron (left half) and
    /// the line-decoration bar (right edge).
    pub decorations: GutterBand,
    pub chevron: GutterBand,
    pub bar: GutterBand,
}

/// One column in the gutter — its left edge (in `GutterContainer`-local
/// pixels) and its width. `width == 0.0` means the band is disabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Reflect)]
#[reflect(Debug, Default, PartialEq)]
pub struct GutterBand {
    pub left: f32,
    pub width: f32,
}

impl GutterBand {
    pub fn right(&self) -> f32 {
        self.left + self.width
    }

    pub fn center(&self) -> f32 {
        self.left + self.width * 0.5
    }

    /// Top-left x of a square icon `size` px wide, centered in this band.
    pub fn place_square(&self, size: f32) -> f32 {
        self.left + (self.width - size) * 0.5
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0
    }
}

impl Default for EditorUi {
    fn default() -> Self {
        Self {
            line_numbers: LineNumbers::On,
            line_numbers_min_chars: 2,
            glyph_margin: true,
            glyph_margin_width: 16.0,
            line_decorations_width: 10.0,
            select_on_line_numbers: true,
            show_gutter: true,
            show_separator: true,
            gutter_padding_left: 0.0,
            gutter_padding_right: 0.0,
            code_margin_left: 0.0,
            placeholder: None,
        }
    }
}

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Indentation {
    pub tab_size: u32,
    pub insert_spaces: bool,
    pub detect_indentation: bool,
    pub indent_size: IndentSize,
    pub use_tab_stops: bool,
    pub sticky_tab_stops: bool,
    pub trim_whitespace_on_delete: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum IndentSize {
    #[default]
    TabSize,
    Cells(u32),
}

impl IndentSize {
    /// Resolve to a concrete column count, falling back to `tab_size`
    /// for [`IndentSize::TabSize`].
    pub fn resolve(self, tab_size: u32) -> usize {
        match self {
            Self::TabSize => tab_size as usize,
            Self::Cells(n) => n as usize,
        }
    }
}

impl Default for Indentation {
    fn default() -> Self {
        Self {
            tab_size: 4,
            insert_spaces: true,
            detect_indentation: true,
            indent_size: IndentSize::TabSize,
            use_tab_stops: true,
            sticky_tab_stops: false,
            trim_whitespace_on_delete: false,
        }
    }
}

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct BracketConfig {
    pub match_brackets: MatchBrackets,
    pub style: BracketHighlightStyle,
    pub colorization: BracketPairColorization,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MatchBrackets {
    Never,
    Near,
    #[default]
    Always,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum BracketHighlightStyle {
    Underline,
    #[default]
    Background,
    Both,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Debug)]
pub struct BracketPairColorization {
    pub enabled: bool,
    pub independent_color_pool_per_type: bool,
}

impl Default for BracketPairColorization {
    fn default() -> Self {
        Self {
            enabled: true,
            independent_color_pool_per_type: false,
        }
    }
}

impl Default for BracketConfig {
    fn default() -> Self {
        Self {
            match_brackets: MatchBrackets::Always,
            style: BracketHighlightStyle::Background,
            colorization: BracketPairColorization::default(),
        }
    }
}
