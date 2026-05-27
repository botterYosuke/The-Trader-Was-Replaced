//! Theme components controlling markdown rendering.
//!
//! Split into four independent `Component`s so callers can override one
//! axis without restating the others. All four are `#[require]`d by
//! [`crate::Markdown`] and fall back to `Default` when not inserted.

use bevy::prelude::*;

/// Font handles. `bold`/`italic`/`bold_italic` fall back to `body` when `None`.
#[derive(Component, Clone, Debug, Default)]
pub struct MarkdownFonts {
    pub body: Handle<Font>,
    pub mono: Handle<Font>,
    pub bold: Option<Handle<Font>>,
    pub italic: Option<Handle<Font>>,
    pub bold_italic: Option<Handle<Font>>,
}

#[derive(Component, Clone, Debug)]
pub struct MarkdownColors {
    pub text: Color,
    pub link: Color,
    pub code_bg: Color,
    /// `Color::NONE` (default) draws no border around code blocks.
    pub code_border: Color,
    pub inline_code_bg: Color,
    pub blockquote_border: Color,
    pub hr: Color,
}

impl Default for MarkdownColors {
    fn default() -> Self {
        Self {
            text: Color::srgb(0.90, 0.90, 0.92),
            link: Color::srgb(0.40, 0.70, 1.00),
            code_bg: Color::srgba(0.10, 0.11, 0.14, 0.85),
            code_border: Color::NONE,
            inline_code_bg: Color::srgba(0.18, 0.20, 0.24, 0.85),
            blockquote_border: Color::srgb(0.45, 0.48, 0.55),
            hr: Color::srgb(0.30, 0.32, 0.38),
        }
    }
}

#[derive(Component, Clone, Debug)]
pub struct MarkdownSpacing {
    pub base_font_size: f32,
    pub line_height_mul: f32,
    pub block_gap: f32,
    pub list_indent: f32,
    pub code_padding: UiRect,
    /// Border radius for fenced code blocks (`0.0` for sharp corners).
    pub code_corner_radius: f32,
    pub blockquote_border_width: f32,
}

impl Default for MarkdownSpacing {
    fn default() -> Self {
        Self {
            base_font_size: 14.0,
            line_height_mul: 1.4,
            block_gap: 8.0,
            list_indent: 20.0,
            code_padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
            code_corner_radius: 4.0,
            blockquote_border_width: 3.0,
        }
    }
}

/// H1..H6 size multipliers applied to [`MarkdownSpacing::base_font_size`].
#[derive(Component, Clone, Debug)]
pub struct MarkdownScales(pub [f32; 6]);

impl Default for MarkdownScales {
    fn default() -> Self {
        Self([2.0, 1.5, 1.25, 1.0, 0.9, 0.85])
    }
}

pub(crate) struct ThemeRef<'a> {
    pub fonts: &'a MarkdownFonts,
    pub colors: &'a MarkdownColors,
    pub spacing: &'a MarkdownSpacing,
    pub scales: &'a MarkdownScales,
}
