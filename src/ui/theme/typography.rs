//! Typography tokens: font sizes, line heights, weights, and families.
//!
//! Modeled after Zed's `crates/ui/src/styles/typography.rs` and TextSize scale.
//! Values are tuned for a desktop trading UI at 1× DPI.
//!
//! IMPORTANT (issue #48 scope): the `mono` `TypeStyle` is declared here but
//! NOT wired to any editor / gutter / orderbook surface. The mono font face
//! and the editor-side application land in #50 (bevscode replacement). This
//! file only defines the tokens.
//!
//! NOTE: Not a Bevy Resource. `Theme` (Step 6) will own a `Typography` field
//! and accessors below take `&self`.

use bevy::asset::Handle;
use bevy::text::{Font, LineHeight, TextFont};

/// Font weight. Numeric repr matches CSS conventions; kept as an internal
/// enum (rather than `bevy::text::FontWeight`) to avoid coupling to Bevy
/// 0.18's evolving text API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u16)]
pub enum FontWeight {
    Normal = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
}

/// Logical font family. Resolution to a concrete `Handle<Font>` happens in
/// the renderer layer (Step 6+).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FontFamily {
    Sans,
    Mono,
}

/// Headline tier. Maps to fixed indices in `Typography::headline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HeadlineSize {
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
}

/// Label tier. Maps to fixed indices in `Typography::label`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LabelSize {
    XSmall,
    Small,
    Default,
    Large,
}

/// A single text style token: size, line height, weight, family.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TypeStyle {
    pub size: f32,
    pub line_height: f32,
    pub weight: FontWeight,
    pub family: FontFamily,
}

impl TypeStyle {
    #[inline]
    pub const fn new(size: f32, line_height: f32, weight: FontWeight, family: FontFamily) -> Self {
        Self { size, line_height, weight, family }
    }

    /// Returns this token's `size` as `TextFont::font_size` and `line_height`
    /// as `LineHeight::Px`. The font handle is left default; concrete font
    /// face resolution lands in #50 (bevscode replacement).
    pub fn text_font(&self) -> (TextFont, LineHeight) {
        (
            TextFont { font_size: self.size, ..Default::default() },
            LineHeight::Px(self.line_height),
        )
    }

    /// Same as `text_font` but binds the given font handle. Used by symbol-only
    /// surfaces (e.g. footer ▶/■) where the default font lacks the glyph but
    /// size / line height must still come from the label token.
    pub fn text_font_with_font(&self, font: Handle<Font>) -> (TextFont, LineHeight) {
        (
            TextFont { font, font_size: self.size, ..Default::default() },
            LineHeight::Px(self.line_height),
        )
    }
}

/// All typography tokens for the active theme.
///
/// Built from `Default::default()` (dark theme baseline). Look up styles via
/// the accessors below; do not index the arrays directly from call sites.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Typography {
    /// XSmall, Small, Medium, Large, XLarge (5 tiers).
    headline: [TypeStyle; 5],
    /// XSmall, Small, Default, Large (4 tiers).
    label: [TypeStyle; 4],
    /// Body / paragraph text.
    pub body: TypeStyle,
    /// Monospace text. Declared only — editor wiring lands in #50.
    pub mono: TypeStyle,
}

impl Typography {
    #[inline]
    pub fn headline(&self, size: HeadlineSize) -> &TypeStyle {
        &self.headline[size as usize]
    }

    #[inline]
    pub fn label(&self, size: LabelSize) -> &TypeStyle {
        &self.label[size as usize]
    }

    /// Delegates to `TypeStyle::text_font` for the requested headline tier.
    #[inline]
    pub fn headline_font(&self, size: HeadlineSize) -> (TextFont, LineHeight) {
        self.headline(size).text_font()
    }

    /// Delegates to `TypeStyle::text_font` for the requested label tier.
    #[inline]
    pub fn label_font(&self, size: LabelSize) -> (TextFont, LineHeight) {
        self.label(size).text_font()
    }

    /// Delegates to `TypeStyle::text_font_with_font` for the requested label tier.
    #[inline]
    pub fn label_font_with_font(
        &self,
        size: LabelSize,
        font: Handle<Font>,
    ) -> (TextFont, LineHeight) {
        self.label(size).text_font_with_font(font)
    }
}

impl Default for Typography {
    fn default() -> Self {
        // Line heights: headline ≈ size × 1.25, body/label ≈ size × 1.4.
        // Values mirror Zed's TextSize scale at desktop DPI.
        Self {
            headline: [
                TypeStyle::new(13.0, 16.0, FontWeight::SemiBold, FontFamily::Sans), // XSmall
                TypeStyle::new(14.0, 18.0, FontWeight::SemiBold, FontFamily::Sans), // Small
                TypeStyle::new(16.0, 20.0, FontWeight::SemiBold, FontFamily::Sans), // Medium
                TypeStyle::new(18.0, 22.0, FontWeight::Bold,     FontFamily::Sans), // Large
                TypeStyle::new(22.0, 28.0, FontWeight::Bold,     FontFamily::Sans), // XLarge
            ],
            label: [
                TypeStyle::new(10.0, 14.0, FontWeight::Normal, FontFamily::Sans), // XSmall
                TypeStyle::new(11.0, 15.0, FontWeight::Normal, FontFamily::Sans), // Small
                TypeStyle::new(12.0, 17.0, FontWeight::Medium, FontFamily::Sans), // Default
                TypeStyle::new(13.0, 18.0, FontWeight::Medium, FontFamily::Sans), // Large
            ],
            body: TypeStyle::new(13.0, 18.0, FontWeight::Normal, FontFamily::Sans),
            mono: TypeStyle::new(12.0, 17.0, FontWeight::Normal, FontFamily::Mono),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::text::LineHeight;

    #[test]
    fn label_font_small_carries_declared_size_and_line_height() {
        let typo = Typography::default();
        let (tf, lh) = typo.label_font(LabelSize::Small);
        assert_eq!(tf.font_size, 11.0);
        assert_eq!(lh, LineHeight::Px(15.0));
    }

    #[test]
    fn headline_font_medium_carries_declared_size_and_line_height() {
        let typo = Typography::default();
        let (tf, lh) = typo.headline_font(HeadlineSize::Medium);
        assert_eq!(tf.font_size, 16.0);
        assert_eq!(lh, LineHeight::Px(20.0));
    }

    #[test]
    fn label_font_with_font_small_carries_size_line_height_and_font() {
        let typo = Typography::default();
        let handle = Handle::<bevy::text::Font>::default();
        let (tf, lh) = typo.label_font_with_font(LabelSize::Small, handle.clone());
        assert_eq!(tf.font_size, 11.0);
        assert_eq!(lh, LineHeight::Px(15.0));
        assert_eq!(tf.font, handle);
    }
}

/// Test-only constructor that mutates every serializable field of
/// `Typography`, including the private `headline` / `label` arrays.
/// Used by `tests/e2e/flows/q3_theme_serde_roundtrip.rs` (M1) to gate
/// `#[serde(skip)]` silent drops.
///
/// `#[doc(hidden)] pub` (not `pub(crate)` / not `#[cfg(test)]`) because
/// integration test targets in `tests/` compile the lib WITHOUT `cfg(test)`.
#[doc(hidden)]
pub fn non_default_typography() -> Typography {
    let mk = |i: f32, family: FontFamily, weight: FontWeight| {
        TypeStyle::new(100.0 + i, 200.0 + i, weight, family)
    };
    Typography {
        headline: [
            mk(1.0, FontFamily::Mono, FontWeight::Normal),
            mk(2.0, FontFamily::Mono, FontWeight::Medium),
            mk(3.0, FontFamily::Mono, FontWeight::Normal),
            mk(4.0, FontFamily::Mono, FontWeight::Medium),
            mk(5.0, FontFamily::Mono, FontWeight::Normal),
        ],
        label: [
            mk(6.0, FontFamily::Mono, FontWeight::Bold),
            mk(7.0, FontFamily::Mono, FontWeight::SemiBold),
            mk(8.0, FontFamily::Mono, FontWeight::Bold),
            mk(9.0, FontFamily::Mono, FontWeight::SemiBold),
        ],
        body: mk(10.0, FontFamily::Mono, FontWeight::Bold),
        mono: mk(11.0, FontFamily::Sans, FontWeight::Bold),
    }
}
