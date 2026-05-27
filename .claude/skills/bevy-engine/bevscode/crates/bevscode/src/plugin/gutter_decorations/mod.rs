//! Gutter decorations: glyph-margin icons, line-decoration bars,
//! fold chevrons. Each lives in its own submodule with a focused sync
//! system; shared pool-and-diff helpers sit in `common`.
//!
//! Host-facing surface:
//! - [`GlyphMarkers`] / [`GlyphMarker`] / [`GlyphKind`] — glyph-margin icons.
//! - [`GutterDecorations`] / [`LineDecoration`] / [`DecorationKind`] — bars.
//! - [`GlyphMarginClicked`] — click events.
//! - [`IconAtlas`] — rasterised SVG handles for hosts that want to
//!   spawn additional decorations sharing the same atlas.
//!
//! Icons ship bundled as Iconoir SVGs (MIT-licensed; see
//! `assets/icons/LICENSE-ICONOIR`), embedded via `include_bytes!` and
//! rasterised once at PreStartup through `bevy_resvg`. Hosts do not
//! need to copy any asset files.

mod bars;
mod chevrons;
mod click;
mod common;
mod icons;
mod markers;

#[cfg(feature = "lsp")]
mod lsp;

pub use self::bars::{
    DecorationKind, GutterDecorationBar, GutterDecorations, LineDecoration, LineDecorationRects,
};
pub use self::chevrons::GutterFoldChevron;
pub use self::click::{GlyphMarginClicked, GlyphMarginRects};
pub use self::icons::IconAtlas;
pub use self::markers::{GlyphKind, GlyphMarker, GlyphMarkers, GutterIcon};

pub use self::click::on_glyph_margin_press;
pub use self::common::buffer_line_at_y;

pub(crate) use self::bars::{sync_gutter_decoration_bars, update_line_decoration_overlays};
pub(crate) use self::chevrons::{drive_chevron_rotation, sync_fold_chevron_icons};
pub(crate) use self::click::update_glyph_margin_overlays;
pub(crate) use self::icons::setup_icon_atlas;
pub(crate) use self::markers::sync_gutter_icons;

#[cfg(feature = "lsp")]
pub(crate) use self::lsp::sync_lsp_glyph_markers;
