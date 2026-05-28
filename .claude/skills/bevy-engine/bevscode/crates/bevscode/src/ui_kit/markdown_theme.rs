//! Adapter: tempera [`PopupChrome`] → `bevy_markdown` theme components.
//!
//! `bevy_markdown` expects four small theme components on each `Markdown`
//! entity. Inside bevscode they're all derived from the live tempera
//! tokens so popup markdown matches the rest of the chrome (font, body
//! foreground color, popover background, accent for links). One call
//! site, kept here so renderers stay one-liners.
//!
//! Tempera's `FontHandle` carries `regular` + optional `bold` only —
//! no italic, no monospace face. We reuse `regular` for both body and
//! mono and let `bevy_text` synthesize italic via skew. Hosts that
//! want a true mono / italic face can spawn their own `MarkdownFonts`
//! Component on the popup entity to override one axis.

use bevy::prelude::*;
use bevy_markdown::{MarkdownColors, MarkdownFonts, MarkdownScales, MarkdownSpacing};

use super::PopupChrome;

/// Build the four `bevy_markdown` theme components from the current
/// [`PopupChrome`]. Returns them as a tuple ready to spread into a
/// `commands.spawn((Markdown { source }, fonts, colors, spacing, scales))`.
pub fn markdown_theme_from_chrome(
    chrome: &PopupChrome<'_>,
) -> (
    MarkdownFonts,
    MarkdownColors,
    MarkdownSpacing,
    MarkdownScales,
) {
    let regular = chrome.font.regular.clone().unwrap_or_default();
    let bold = chrome.font.bold.clone().or(Some(regular.clone()));

    let fonts = MarkdownFonts {
        body: regular.clone(),
        // No mono face in tempera; reuse regular. Fenced code blocks
        // still get the code background + slightly muted color so they
        // remain visually distinct without a separate face.
        mono: regular,
        bold,
        italic: None,
        bold_italic: None,
    };

    // Fenced code blocks in hover popups render with no background or
    // border — tree-sitter highlighting from `MarkdownHighlighter` is
    // the only thing that visually distinguishes them from body prose.
    // Matches the editor's own buffer rendering, where syntax color
    // alone carries "this is code."
    //
    // `link` uses `accent_foreground` rather than `primary` because
    // tempera dark themes ship `primary = white`, which is invisibly
    // bright against the popover background.
    let colors = MarkdownColors {
        text: chrome.palette.popover_foreground,
        link: chrome.palette.accent_foreground,
        code_bg: Color::NONE,
        code_border: Color::NONE,
        // `inline_code_bg` is the foreground *color* for inline code
        // spans here (the engine has no per-span background in 0.18),
        // so it must be a readable text color. Popover foreground keeps
        // inline code legible — `bevy_markdown` still picks the mono
        // face so it remains visually distinct.
        inline_code_bg: chrome.palette.popover_foreground,
        blockquote_border: chrome.palette.accent,
        hr: chrome.palette.border,
    };

    let spacing = MarkdownSpacing {
        // Anchor to typography `base` (body) so heading scales produce
        // real size steps. `sm` would compress every heading to body-
        // sized text.
        base_font_size: chrome.typography.base,
        line_height_mul: 1.4,
        block_gap: chrome.spacing.xs,
        list_indent: chrome.spacing.md,
        // No background ⇒ no padding. The block-margin above/below
        // (`block_gap`) is still applied by `spawn_code_block` so the
        // code sits as its own paragraph rather than running into the
        // surrounding prose.
        code_padding: UiRect::ZERO,
        code_corner_radius: 0.0,
        blockquote_border_width: 2.0,
    };

    // Tighter heading scales than the [`MarkdownScales::default`]
    // (`[2.0, 1.5, 1.25, 1.0, 0.9, 0.85]`). At `base = 14px` a 2.0×
    // h1 is 28px — too dominant for a 320px-capped hover chrome,
    // especially since rust-analyzer hovers usually open with a
    // single h2/h3 header followed by code. These steps stay close
    // to body size while still ranking h1..h6 distinguishably.
    let scales = MarkdownScales([1.15, 1.1, 1.05, 1.0, 0.95, 0.9]);

    (fonts, colors, spacing, scales)
}
