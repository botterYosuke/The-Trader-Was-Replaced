//! Radix-inspired 12-step color scale, dark variant only.
//!
//! Modeled after Zed's `crates/theme/src/scale.rs` and the Radix UI color system.
//! Each step has a fixed semantic role (background → border → solid → text).
//! Dark light variant only in this issue (#48); the light variant lands later.
//!
//! Step semantics (Radix):
//!   1  app background
//!   2  subtle background (striped tables, sidebars, cards)
//!   3  ui element background (normal)
//!   4  ui element background (hover)
//!   5  ui element background (pressed / selected)
//!   6  subtle border / divider (non-interactive)
//!   7  ui element border / focused field border (interactive)
//!   8  stronger border, focus ring
//!   9  solid background (brand / accent fill)
//!   10 solid background (hover)
//!   11 low-contrast text
//!   12 high-contrast text

use bevy::prelude::*;

/// 12-step Radix-style color scale. Indexing is 1-based externally,
/// 0-based internally. Construct via [`ColorScale::neutral_dark`] etc.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ColorScale([Color; 12]);

impl ColorScale {
    /// Build from an explicit 12-color array (steps 1..=12).
    #[inline]
    pub const fn new(steps: [Color; 12]) -> Self {
        Self(steps)
    }

    /// Returns step N (1-based). Panics in debug if `step` is not in 1..=12.
    #[inline]
    pub const fn step(&self, step: usize) -> Color {
        self.0[step - 1]
    }

    #[inline] pub const fn step_1(&self)  -> Color { self.0[0]  }
    #[inline] pub const fn step_2(&self)  -> Color { self.0[1]  }
    #[inline] pub const fn step_3(&self)  -> Color { self.0[2]  }
    #[inline] pub const fn step_4(&self)  -> Color { self.0[3]  }
    #[inline] pub const fn step_5(&self)  -> Color { self.0[4]  }
    #[inline] pub const fn step_6(&self)  -> Color { self.0[5]  }
    #[inline] pub const fn step_7(&self)  -> Color { self.0[6]  }
    #[inline] pub const fn step_8(&self)  -> Color { self.0[7]  }
    #[inline] pub const fn step_9(&self)  -> Color { self.0[8]  }
    #[inline] pub const fn step_10(&self) -> Color { self.0[9]  }
    #[inline] pub const fn step_11(&self) -> Color { self.0[10] }
    #[inline] pub const fn step_12(&self) -> Color { self.0[11] }
}

// -- Dark scales ------------------------------------------------------------
//
// `neutral_dark` is filled with Radix `slate` dark values (approximated to
// sRGB). Other accent scales currently only define steps 9/11/12 with real
// brand values; steps 1..=8 and 10 fall back to neutral_dark so the API is
// usable today. Step 9 of this issue (footer token-isation) will surface the
// exact accent values it needs; we will tighten the scales then.

impl ColorScale {
    /// Neutral (slate) dark scale.
    pub const fn neutral_dark() -> Self {
        Self::new([
            Color::srgb(0.0706, 0.0784, 0.0902), //  1  #121316 app bg
            Color::srgb(0.0941, 0.1020, 0.1176), //  2  #181a1e subtle bg
            Color::srgb(0.1255, 0.1373, 0.1569), //  3  #202328 ui bg
            Color::srgb(0.1529, 0.1686, 0.1922), //  4  #272b31 ui hover
            Color::srgb(0.1843, 0.2039, 0.2314), //  5  #2f343b ui pressed
            Color::srgb(0.2235, 0.2471, 0.2784), //  6  #393f47 subtle border
            Color::srgb(0.2745, 0.3020, 0.3373), //  7  #464d56 ui border
            Color::srgb(0.3608, 0.3922, 0.4314), //  8  #5c646e strong border
            Color::srgb(0.4314, 0.4627, 0.5020), //  9  #6e7680 solid
            Color::srgb(0.4824, 0.5137, 0.5529), // 10  #7b838d solid hover
            Color::srgb(0.7059, 0.7255, 0.7529), // 11  #b4b9c0 low-contrast text
            Color::srgb(0.9255, 0.9333, 0.9451), // 12  #ecedf1 high-contrast text
        ])
    }

    /// Accent (blue) dark scale — only steps 9/11/12 are brand-coloured;
    /// other steps mirror `neutral_dark` until Step 9 of #48 needs them.
    pub const fn accent_dark() -> Self {
        let n = Self::neutral_dark();
        Self::new([
            n.0[0], n.0[1], n.0[2], n.0[3], n.0[4], n.0[5], n.0[6], n.0[7],
            Color::srgb(0.235, 0.510, 0.965), //  9 brand solid (Radix blue 9)
            n.0[9],
            Color::srgb(0.451, 0.682, 1.000), // 11 brand text (Radix blue 11)
            Color::srgb(0.792, 0.890, 1.000), // 12 brand high-contrast
        ])
    }

    /// Red (danger) dark scale.
    pub const fn red_dark() -> Self {
        let n = Self::neutral_dark();
        Self::new([
            n.0[0], n.0[1], n.0[2], n.0[3], n.0[4], n.0[5], n.0[6], n.0[7],
            Color::srgb(0.882, 0.235, 0.314), //  9 red 9
            n.0[9],
            Color::srgb(1.000, 0.467, 0.510), // 11
            Color::srgb(1.000, 0.792, 0.804), // 12
        ])
    }

    /// Green (success) dark scale.
    pub const fn green_dark() -> Self {
        let n = Self::neutral_dark();
        Self::new([
            n.0[0], n.0[1], n.0[2], n.0[3], n.0[4], n.0[5], n.0[6], n.0[7],
            Color::srgb(0.180, 0.706, 0.439), //  9 green 9
            n.0[9],
            Color::srgb(0.392, 0.851, 0.580), // 11
            Color::srgb(0.745, 0.949, 0.812), // 12
        ])
    }

    /// Yellow (warning) dark scale.
    pub const fn yellow_dark() -> Self {
        let n = Self::neutral_dark();
        Self::new([
            n.0[0], n.0[1], n.0[2], n.0[3], n.0[4], n.0[5], n.0[6], n.0[7],
            Color::srgb(0.961, 0.792, 0.090), //  9 yellow 9
            n.0[9],
            Color::srgb(0.965, 0.851, 0.349), // 11
            Color::srgb(1.000, 0.945, 0.749), // 12
        ])
    }

    /// Blue (info) dark scale — distinct from `accent_dark` for status use.
    pub const fn blue_dark() -> Self {
        let n = Self::neutral_dark();
        Self::new([
            n.0[0], n.0[1], n.0[2], n.0[3], n.0[4], n.0[5], n.0[6], n.0[7],
            Color::srgb(0.235, 0.510, 0.965), //  9
            n.0[9],
            Color::srgb(0.451, 0.682, 1.000), // 11
            Color::srgb(0.792, 0.890, 1.000), // 12
        ])
    }
}
