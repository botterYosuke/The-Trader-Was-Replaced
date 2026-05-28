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

/// Font weight. Numeric repr matches CSS conventions; kept as an internal
/// enum (rather than `bevy::text::FontWeight`) to avoid coupling to Bevy
/// 0.18's evolving text API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum FontWeight {
    Normal = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
}

/// Logical font family. Resolution to a concrete `Handle<Font>` happens in
/// the renderer layer (Step 6+).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontFamily {
    Sans,
    Mono,
}

/// Headline tier. Maps to fixed indices in `Typography::headline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlineSize {
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
}

/// Label tier. Maps to fixed indices in `Typography::label`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelSize {
    XSmall,
    Small,
    Default,
    Large,
}

/// A single text style token: size, line height, weight, family.
#[derive(Debug, Clone, Copy)]
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
}

/// All typography tokens for the active theme.
///
/// Built from `Default::default()` (dark theme baseline). Look up styles via
/// the accessors below; do not index the arrays directly from call sites.
#[derive(Debug, Clone, Copy)]
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
