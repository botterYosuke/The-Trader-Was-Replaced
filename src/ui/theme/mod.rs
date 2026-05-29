//! Design-system root module for the UI theme.
//!
//! Issue #48 — Step 6: this module now hosts the complete [`Theme`] Resource
//! and its supporting value types ([`ThemeColors`], [`StatusColors`],
//! [`SyntaxColors`], [`PlayerColors`], [`Radius`], [`Layout`], [`Appearance`]).
//! Submodule re-exports below let call sites import everything from
//! `crate::ui::theme::*`.
//!
//! Scope guards (from the approved plan):
//! * `SyntaxColors` declares fields only; syntect / tree-sitter conversion
//!   impls are deferred to #50.
//! * Light variant and JSON loading are out of scope; `Default for Theme`
//!   builds the dark variant from `ColorScale::*_dark()`.
//! * `InputPhase` SystemSet and `mono` editor wiring are out of scope (#50).

use bevy::prelude::*;

pub mod scale;
pub mod spacing;
pub mod typography;
pub mod elevation;

pub use elevation::ElevationIndex;
pub use scale::ColorScale;
pub use spacing::{DynamicSpacing, UiDensity};
pub use typography::{
    FontFamily, FontWeight, HeadlineSize, LabelSize, TypeStyle, Typography,
};

// -- ThemeColors ------------------------------------------------------------

/// Semantic UI colors derived from [`ColorScale`] steps.
///
/// Field naming mirrors Zed's `ThemeColors`. Each field has a single
/// canonical use site; do not reach into [`ColorScale`] directly from UI code
/// unless a new semantic role is being added here.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ThemeColors {
    /// Root window / app background (scale step 1).
    pub background: Color,
    /// Default panel / surface background (scale step 2).
    pub surface_background: Color,
    /// Floating surface (popover, dropdown, tooltip) background (step 3).
    pub elevated_surface_background: Color,
    /// Sidebar / footer / dock panel background (step 2).
    pub panel_background: Color,

    /// Subtle non-interactive border / divider (step 6).
    pub border: Color,
    /// Slightly stronger border variant (step 7).
    pub border_variant: Color,
    /// Focused field / focus ring border (step 8).
    pub border_focused: Color,

    /// High-contrast body text (step 12).
    pub text: Color,
    /// Low-contrast / secondary text (step 11).
    pub text_muted: Color,
    /// Placeholder text inside inputs (step 9 of neutral).
    pub text_placeholder: Color,
    /// Disabled text (step 8).
    pub text_disabled: Color,
    /// Accent-colored text (accent step 11).
    pub text_accent: Color,

    /// Default interactive element background (step 3).
    pub element_background: Color,
    /// Hover state for interactive elements (step 4).
    pub element_hover: Color,
    /// Pressed / active state (step 5).
    pub element_active: Color,
    /// Selected state (step 5 + slight accent tint via accent step 5).
    pub element_selected: Color,

    /// Accent solid fill (accent step 9).
    pub accent: Color,
    /// Accent solid fill, hover (accent step 10).
    pub accent_hover: Color,

    /// Default icon color (step 11).
    pub icon: Color,
    /// Muted / secondary icon (step 8).
    pub icon_muted: Color,
    /// Disabled icon (step 7).
    pub icon_disabled: Color,
    /// Accent icon (accent step 11).
    pub icon_accent: Color,
}

// -- StatusColors -----------------------------------------------------------

/// Status / semantic colors (info / warning / error / success) and trading-
/// specific roles (long / short / bid / ask). Each role exposes a solid color
/// plus matching `_background` and `_border` variants.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StatusColors {
    pub info: Color,
    pub info_background: Color,
    pub info_border: Color,

    pub warning: Color,
    pub warning_background: Color,
    pub warning_border: Color,

    pub error: Color,
    pub error_background: Color,
    pub error_border: Color,

    pub success: Color,
    pub success_background: Color,
    pub success_border: Color,

    /// Long position / buy side (green family).
    pub long: Color,
    pub long_background: Color,
    pub long_border: Color,

    /// Short position / sell side (red family).
    pub short: Color,
    pub short_background: Color,
    pub short_border: Color,

    /// Orderbook bid side (green family, slightly desaturated).
    pub bid: Color,
    pub bid_background: Color,
    pub bid_border: Color,

    /// Orderbook ask side (red family, slightly desaturated).
    pub ask: Color,
    pub ask_background: Color,
    pub ask_border: Color,
}

// -- SyntaxColors -----------------------------------------------------------

/// Syntax highlighting palette. **Fields only** in #48 — the conversion to /
/// from `syntect::Theme` (or tree-sitter highlight queries) lands in #50.
///
/// NOTE: `type_` has a trailing underscore to avoid the reserved word `type`.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SyntaxColors {
    /// Line / block comments.
    pub comment: Color,
    /// Language keywords (`fn`, `let`, `if`, ...).
    pub keyword: Color,
    /// String literals.
    pub string: Color,
    /// Numeric literals.
    pub number: Color,
    /// Type names and built-in types (trailing `_` avoids the `type` keyword).
    pub type_: Color,
    /// Function names at definition and call sites.
    pub function: Color,
    /// Variable / identifier references.
    pub variable: Color,
    /// Operators and punctuation.
    pub operator: Color,
}

// -- PlayerColors -----------------------------------------------------------

/// Distinct chart / series palette (8 colors). Index by series number; wrap
/// modulo `8` for more than 8 series.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlayerColors(pub [Color; 8]);

impl PlayerColors {
    #[inline]
    pub const fn get(&self, index: usize) -> Color {
        self.0[index % 8]
    }
}

// -- Radius -----------------------------------------------------------------

/// Border-radius tokens, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Radius {
    /// Small (2 px) — chips, tags.
    pub sm: f32,
    /// Medium (4 px) — buttons, inputs, default surfaces.
    pub md: f32,
    /// Large (8 px) — modals, cards.
    pub lg: f32,
    /// Pill / fully rounded (effectively `f32::INFINITY` in CSS).
    pub full: f32,
}

impl Default for Radius {
    fn default() -> Self {
        Self { sm: 2.0, md: 4.0, lg: 8.0, full: 9999.0 }
    }
}

// -- Layout -----------------------------------------------------------------

/// Layout tokens that are global to the whole app (chrome sizes).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Layout {
    /// Toolbar / menu bar height in px.
    pub toolbar_h: f32,
    /// Footer / status bar height in px.
    pub footer_h: f32,
    /// Default sidebar width in px.
    pub sidebar_w: f32,
    /// Default inspector / right panel width in px.
    pub inspector_w: f32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            toolbar_h: 32.0,
            footer_h: 24.0,
            sidebar_w: 240.0,
            inspector_w: 280.0,
        }
    }
}

// -- Appearance -------------------------------------------------------------

/// Light / dark appearance flag. Only `Dark` is fully populated in #48.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}

// -- ColorScales -----------------------------------------------------------

/// All Radix-style 12-step scales used by the theme. Wraps six [`ColorScale`]
/// instances under semantic names so call sites can write
/// `theme.scale.accent.step_9()` instead of reaching into [`ColorScale::*_dark`]
/// constructors directly.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ColorScales {
    pub neutral: ColorScale,
    pub accent: ColorScale,
    pub red: ColorScale,
    pub green: ColorScale,
    pub yellow: ColorScale,
    pub blue: ColorScale,
}

impl Default for ColorScales {
    /// `Default` builds the dark variant; light variant lands later (out of scope for #48).
    fn default() -> Self {
        Self {
            neutral: ColorScale::neutral_dark(),
            accent: ColorScale::accent_dark(),
            red: ColorScale::red_dark(),
            green: ColorScale::green_dark(),
            yellow: ColorScale::yellow_dark(),
            blue: ColorScale::blue_dark(),
        }
    }
}

// -- SpacingTokens ----------------------------------------------------------

/// Density-aware spacing resolver. Owns the active [`UiDensity`] and exposes
/// `px(t)` so call sites can write `theme.spacing.px(DynamicSpacing::Base08)`
/// without threading density through every signature.
///
/// `SpacingTokens::density` is the single source of truth for UI density;
/// `Layout` no longer carries a density field.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct SpacingTokens {
    pub density: UiDensity,
}

impl SpacingTokens {
    #[inline]
    pub fn px(&self, t: DynamicSpacing) -> f32 {
        t.px(self.density)
    }
}

// -- ElevationTokens --------------------------------------------------------

/// Zero-sized handle that re-exposes [`ElevationIndex::z`] and
/// [`ElevationIndex::background`] through the theme so call sites can write
/// `theme.elevation.z(ElevationIndex::ModalSurface)` and
/// `theme.elevation.background(ElevationIndex::Surface, &theme.colors)`
/// without importing `ElevationIndex` separately at every site.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ElevationTokens;

impl ElevationTokens {
    #[inline]
    pub fn z(&self, tier: ElevationIndex) -> f32 {
        tier.z()
    }

    #[inline]
    pub fn background(&self, tier: ElevationIndex, colors: &ThemeColors) -> Color {
        tier.background_for_colors(colors)
    }
}

// -- Theme ------------------------------------------------------------------

/// Design-system root Resource. Built by `Default::default()` (dark variant);
/// runtime theme swapping lands later.
#[derive(Resource, Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    pub colors: ThemeColors,
    pub status: StatusColors,
    pub syntax: SyntaxColors,
    pub players: PlayerColors,
    pub scale: ColorScales,
    pub spacing: SpacingTokens,
    pub typography: Typography,
    pub elevation: ElevationTokens,
    pub radius: Radius,
    pub layout: Layout,
    pub appearance: Appearance,
}

impl ThemeColors {
    /// Derive every semantic UI color from a [`ColorScales`] palette.
    ///
    /// Issue #48 H3: this is the single source of truth for the
    /// scale-step → semantic-role mapping. `Theme::dark()` / `Theme::light()`
    /// (M7) will reuse this by passing a different `ColorScales`.
    pub fn from_scales(s: &ColorScales) -> Self {
        let neutral = &s.neutral;
        let accent = &s.accent;
        Self {
            background: neutral.step_1(),
            surface_background: neutral.step_2(),
            elevated_surface_background: neutral.step_3(),
            panel_background: neutral.step_2(),

            border: neutral.step_6(),
            border_variant: neutral.step_7(),
            border_focused: accent.step_8(),

            text: neutral.step_12(),
            text_muted: neutral.step_11(),
            text_placeholder: neutral.step_9(),
            text_disabled: neutral.step_8(),
            text_accent: accent.step_11(),

            element_background: neutral.step_3(),
            element_hover: neutral.step_4(),
            element_active: neutral.step_5(),
            element_selected: accent.step_5(),

            accent: accent.step_9(),
            accent_hover: accent.step_10(),

            icon: neutral.step_11(),
            icon_muted: neutral.step_8(),
            icon_disabled: neutral.step_7(),
            icon_accent: accent.step_11(),
        }
    }
}

impl StatusColors {
    /// Derive info / warning / error / success / long / short / bid / ask
    /// from a [`ColorScales`] palette.
    pub fn from_scales(s: &ColorScales) -> Self {
        let blue = &s.blue;
        let yellow = &s.yellow;
        let red = &s.red;
        let green = &s.green;
        Self {
            info: blue.step_9(),
            info_background: blue.step_3(),
            info_border: blue.step_7(),

            warning: yellow.step_9(),
            warning_background: yellow.step_3(),
            warning_border: yellow.step_7(),

            error: red.step_9(),
            error_background: red.step_3(),
            error_border: red.step_7(),

            success: green.step_9(),
            success_background: green.step_3(),
            success_border: green.step_7(),

            long: green.step_9(),
            long_background: green.step_3(),
            long_border: green.step_7(),

            short: red.step_9(),
            short_background: red.step_3(),
            short_border: red.step_7(),

            bid: green.step_11(),
            bid_background: green.step_2(),
            bid_border: green.step_6(),

            ask: red.step_11(),
            ask_background: red.step_2(),
            ask_border: red.step_6(),
        }
    }
}

impl SyntaxColors {
    /// Placeholder dark palette derived from [`ColorScales`]. Real values
    /// land in #50 alongside the syntect / tree-sitter integration.
    pub fn from_scales(s: &ColorScales) -> Self {
        let neutral = &s.neutral;
        let accent = &s.accent;
        let green = &s.green;
        let yellow = &s.yellow;
        let blue = &s.blue;
        Self {
            comment: neutral.step_8(),
            keyword: accent.step_11(),
            string: green.step_11(),
            number: yellow.step_11(),
            type_: accent.step_12(),
            function: blue.step_11(),
            variable: neutral.step_12(),
            operator: neutral.step_11(),
        }
    }
}

impl PlayerColors {
    /// Chart series palette: mix of step 9 (strong) and step 11 (muted)
    /// across accent / green / yellow / red for 8 distinct hues.
    pub fn from_scales(s: &ColorScales) -> Self {
        let accent = &s.accent;
        let green = &s.green;
        let yellow = &s.yellow;
        let red = &s.red;
        Self([
            accent.step_9(),
            green.step_9(),
            yellow.step_9(),
            red.step_9(),
            accent.step_11(),
            green.step_11(),
            yellow.step_11(),
            red.step_11(),
        ])
    }
}

impl Theme {
    /// Build a [`Theme`] from a single [`ColorScales`] palette. Swapping
    /// `ColorScales::dark()` ↔ `ColorScales::light()` (M7) is the only
    /// switch needed for appearance.
    ///
    /// Note: `appearance` is set to [`Appearance::Dark`] here because the
    /// scale identity cannot be inferred at the type level. `Theme::dark()`
    /// / `Theme::light()` (M7) override this after `from_scales`.
    pub fn from_scales(scales: ColorScales) -> Self {
        Self {
            colors: ThemeColors::from_scales(&scales),
            status: StatusColors::from_scales(&scales),
            syntax: SyntaxColors::from_scales(&scales),
            players: PlayerColors::from_scales(&scales),
            scale: scales,
            spacing: SpacingTokens::default(),
            typography: Typography::default(),
            elevation: ElevationTokens::default(),
            radius: Radius::default(),
            layout: Layout::default(),
            appearance: Appearance::Dark,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::from_scales(ColorScales::default())
    }
}

// -- ActiveTheme ------------------------------------------------------------

/// Extension trait for ergonomic `&Theme` access from `World` / `App`.
///
/// Inside systems, prefer `Res<Theme>` directly. This trait exists for the
/// non-system contexts (one-shot setup, tests, debug tooling).
pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for World {
    #[inline]
    fn theme(&self) -> &Theme {
        self.resource::<Theme>()
    }
}

impl ActiveTheme for App {
    #[inline]
    fn theme(&self) -> &Theme {
        self.world().resource::<Theme>()
    }
}

/// Issue #48 Finding 3: 単一のテスト/本番配線ポイントで `Theme` Resource を
/// 初期化するための薄い Plugin。`UiPlugin` と各 footer e2e harness から
/// 同一の `app.add_plugins(ThemePlugin)` で参照する。
///
/// Theme 依存 system が追加された際は、このプラグインに集約する
/// （`init_resource::<Theme>()` を各所で直書きしない）。
pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Theme>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slice 3a RED: `theme.scale.accent` must carry the real
    /// `ColorScale::accent_dark()` palette, not the neutral stub. Step 9 of
    /// accent is brand-blue `srgb(0.235, 0.510, 0.965)`; neutral step 9 is
    /// `srgb(0.4314, 0.4627, 0.5020)`. Slice 3b flips this to green by
    /// implementing the real `ColorScales::default`.
    #[test]
    fn theme_scale_accent_step_9_is_brand_blue() {
        let theme = Theme::default();
        assert_eq!(
            theme.scale.accent.step_9(),
            ColorScale::accent_dark().step_9(),
            "Theme::default().scale.accent should expose ColorScale::accent_dark, not the neutral stub"
        );
    }

    /// `Theme::default().spacing.density` defaults to `UiDensity::Default`.
    /// `SpacingTokens::density` is the single source of truth for UI density.
    #[test]
    fn theme_spacing_density_defaults_to_default() {
        let theme = Theme::default();
        assert_eq!(theme.spacing.density, UiDensity::Default);
    }
}
