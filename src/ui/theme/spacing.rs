//! UI density and dynamic spacing tokens.
//!
//! `UiDensity` selects one of three scales (Compact / Default / Comfortable)
//! so the entire UI can be rescaled by swapping a single field on `Theme`
//! without rebuilding the theme. `DynamicSpacing` is a Radix-style fixed
//! token enum; call `.px(density)` to resolve to a concrete `f32` pixel
//! value at use sites.
//!
//! Modeled after Zed's `crates/ui/src/styles/spacing.rs`.
//! Default values are the canonical scale; Compact ≈ Default × 0.75,
//! Comfortable ≈ Default × 1.25 (rounded to whole pixels).
//!
//! NOTE: Not registered as a Bevy Resource here. `Theme` (issue #48 Step 6)
//! will own the active `UiDensity` and call `DynamicSpacing::px` at draw time.

/// UI density preset. Controls the active column of `DynamicSpacing::px`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum UiDensity {
    Compact,
    #[default]
    Default,
    Comfortable,
}

/// Fixed spacing tokens, resolved to pixels via [`DynamicSpacing::px`].
///
/// Numeric suffix encodes the Default-density pixel value (e.g. `Base08` = 8 px
/// at Default density). Use these tokens in UI code instead of raw `Val::Px`
/// literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DynamicSpacing {
    Base00,
    Base02,
    Base04,
    Base06,
    Base08,
    Base12,
    Base16,
    Base20,
    Base24,
    Base32,
    Base40,
    Base48,
}

impl DynamicSpacing {
    /// Resolve this token to a concrete pixel value at the given density.
    #[inline]
    pub const fn px(self, density: UiDensity) -> f32 {
        // Tuple order: (compact, default, comfortable).
        // Compact ≈ default × 0.75, Comfortable ≈ default × 1.25, rounded.
        let (compact, default, comfortable) = match self {
            DynamicSpacing::Base00 => (0.0, 0.0, 0.0),
            DynamicSpacing::Base02 => (2.0, 2.0, 3.0),
            DynamicSpacing::Base04 => (3.0, 4.0, 5.0),
            DynamicSpacing::Base06 => (4.0, 6.0, 8.0),
            DynamicSpacing::Base08 => (6.0, 8.0, 10.0),
            DynamicSpacing::Base12 => (9.0, 12.0, 15.0),
            DynamicSpacing::Base16 => (12.0, 16.0, 20.0),
            DynamicSpacing::Base20 => (15.0, 20.0, 25.0),
            DynamicSpacing::Base24 => (18.0, 24.0, 30.0),
            DynamicSpacing::Base32 => (24.0, 32.0, 40.0),
            DynamicSpacing::Base40 => (30.0, 40.0, 50.0),
            DynamicSpacing::Base48 => (36.0, 48.0, 60.0),
        };
        match density {
            UiDensity::Compact => compact,
            UiDensity::Default => default,
            UiDensity::Comfortable => comfortable,
        }
    }
}
