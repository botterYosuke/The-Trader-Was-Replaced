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
// All scales are fully populated from Radix dark palettes:
//   neutral_dark → slate, accent_dark → iris, red_dark → red,
//   green_dark → grass, yellow_dark → amber, blue_dark → blue.
// sRGB floats are the published hex values divided by 255 to ~4 sig figs.

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

    /// Accent (iris) dark scale — full 12-step palette from Radix iris dark.
    pub const fn accent_dark() -> Self {
        Self::new([
            Color::srgb(0.0745, 0.0745, 0.1176), //  1  #13131e app bg
            Color::srgb(0.0902, 0.0863, 0.1451), //  2  #171625 subtle bg
            Color::srgb(0.1255, 0.1255, 0.2392), //  3  #20203d ui bg
            Color::srgb(0.1490, 0.1490, 0.3216), //  4  #262652 ui hover
            Color::srgb(0.1765, 0.1725, 0.4000), //  5  #2d2c66 ui pressed
            Color::srgb(0.2196, 0.2118, 0.4745), //  6  #383679 subtle border
            Color::srgb(0.2706, 0.2627, 0.5373), //  7  #454389 ui border
            Color::srgb(0.3412, 0.3255, 0.7765), //  8  #5753c6 strong border
            Color::srgb(0.3569, 0.3569, 0.8392), //  9  #5b5bd6 brand solid
            Color::srgb(0.4314, 0.4157, 0.8706), // 10  #6e6ade solid hover
            Color::srgb(0.6941, 0.6627, 1.0000), // 11  #b1a9ff low-contrast text
            Color::srgb(0.8784, 0.8745, 1.0000), // 12  #e0dfff high-contrast text
        ])
    }

    /// Red (danger) dark scale — full 12-step palette from Radix red dark.
    pub const fn red_dark() -> Self {
        Self::new([
            Color::srgb(0.0980, 0.0667, 0.0667), //  1  #191111 app bg
            Color::srgb(0.1255, 0.0745, 0.0784), //  2  #201314 subtle bg
            Color::srgb(0.2314, 0.0706, 0.0980), //  3  #3b1219 ui bg
            Color::srgb(0.3137, 0.0588, 0.1098), //  4  #500f1c ui hover
            Color::srgb(0.3804, 0.0863, 0.1373), //  5  #611623 ui pressed
            Color::srgb(0.4471, 0.1373, 0.1765), //  6  #72232d subtle border
            Color::srgb(0.5490, 0.2000, 0.2275), //  7  #8c333a ui border
            Color::srgb(0.7098, 0.2706, 0.2824), //  8  #b54548 strong border
            Color::srgb(0.8980, 0.2824, 0.3020), //  9  #e5484d brand solid
            Color::srgb(0.9255, 0.3647, 0.3686), // 10  #ec5d5e solid hover
            Color::srgb(1.0000, 0.5843, 0.5725), // 11  #ff9592 low-contrast text
            Color::srgb(1.0000, 0.8196, 0.8510), // 12  #ffd1d9 high-contrast text
        ])
    }

    /// Green (success) dark scale — full 12-step palette from Radix grass dark.
    pub const fn green_dark() -> Self {
        Self::new([
            Color::srgb(0.0549, 0.0824, 0.0667), //  1  #0e1511 app bg
            Color::srgb(0.0784, 0.1020, 0.0824), //  2  #141a15 subtle bg
            Color::srgb(0.1059, 0.1647, 0.1176), //  3  #1b2a1e ui bg
            Color::srgb(0.1137, 0.2275, 0.1412), //  4  #1d3a24 ui hover
            Color::srgb(0.1451, 0.2824, 0.1765), //  5  #25482d ui pressed
            Color::srgb(0.1765, 0.3412, 0.2118), //  6  #2d5736 subtle border
            Color::srgb(0.2118, 0.4039, 0.2510), //  7  #366740 ui border
            Color::srgb(0.2431, 0.4745, 0.2863), //  8  #3e7949 strong border
            Color::srgb(0.2745, 0.6549, 0.3451), //  9  #46a758 brand solid
            Color::srgb(0.3255, 0.7020, 0.3961), // 10  #53b365 solid hover
            Color::srgb(0.4431, 0.8157, 0.5137), // 11  #71d083 low-contrast text
            Color::srgb(0.7608, 0.9412, 0.7608), // 12  #c2f0c2 high-contrast text
        ])
    }

    /// Yellow (warning) dark scale — full 12-step palette from Radix amber dark.
    pub const fn yellow_dark() -> Self {
        Self::new([
            Color::srgb(0.0863, 0.0706, 0.0471), //  1  #16120c app bg
            Color::srgb(0.1137, 0.0941, 0.0588), //  2  #1d180f subtle bg
            Color::srgb(0.1882, 0.1255, 0.0314), //  3  #302008 ui bg
            Color::srgb(0.2471, 0.1529, 0.0000), //  4  #3f2700 ui hover
            Color::srgb(0.3020, 0.1882, 0.0000), //  5  #4d3000 ui pressed
            Color::srgb(0.3608, 0.2392, 0.0196), //  6  #5c3d05 subtle border
            Color::srgb(0.4431, 0.3098, 0.0980), //  7  #714f19 ui border
            Color::srgb(0.5608, 0.3922, 0.1412), //  8  #8f6424 strong border
            Color::srgb(1.0000, 0.7725, 0.2392), //  9  #ffc53d brand solid
            Color::srgb(1.0000, 0.8392, 0.0392), // 10  #ffd60a solid hover
            Color::srgb(1.0000, 0.7922, 0.0863), // 11  #ffca16 low-contrast text
            Color::srgb(1.0000, 0.9059, 0.7020), // 12  #ffe7b3 high-contrast text
        ])
    }

    /// Blue (info) dark scale — full 12-step palette from Radix blue dark.
    /// Distinct from `accent_dark` (iris) so info status doesn't collide with brand.
    pub const fn blue_dark() -> Self {
        Self::new([
            Color::srgb(0.0510, 0.0824, 0.1255), //  1  #0d1520 app bg
            Color::srgb(0.0667, 0.0980, 0.1529), //  2  #111927 subtle bg
            Color::srgb(0.0510, 0.1569, 0.2784), //  3  #0d2847 ui bg
            Color::srgb(0.0000, 0.2000, 0.3843), //  4  #003362 ui hover
            Color::srgb(0.0000, 0.2510, 0.4549), //  5  #004074 ui pressed
            Color::srgb(0.0627, 0.3020, 0.5294), //  6  #104d87 subtle border
            Color::srgb(0.1255, 0.3647, 0.6196), //  7  #205d9e ui border
            Color::srgb(0.1569, 0.4392, 0.7412), //  8  #2870bd strong border
            Color::srgb(0.0000, 0.5647, 1.0000), //  9  #0090ff brand solid
            Color::srgb(0.2314, 0.6196, 1.0000), // 10  #3b9eff solid hover
            Color::srgb(0.4392, 0.7216, 1.0000), // 11  #70b8ff low-contrast text
            Color::srgb(0.7608, 0.9020, 1.0000), // 12  #c2e6ff high-contrast text
        ])
    }
}
