//! Z-order elevation tokens.
//!
//! Modeled after Zed's `crates/ui/src/styles/elevation.rs`. Every surface
//! that participates in z-ordering should pick an `ElevationIndex` rather
//! than writing a raw `z` literal. Gaps between tiers (10 → 100 → 300 …)
//! leave room for child z-offsets inside the same elevation tier.
//!
//! NOTE (issue #48): a `fn background(theme: &Theme) -> Color` accessor is
//! intentionally NOT defined here — `Theme` / `ThemeColors` does not exist
//! yet. It will be added in Step 6 once `ThemeColors` lands, then call sites
//! can do `ElevationIndex::ModalSurface.background(theme)`.
//
// TODO(#48 Step 6): add `pub fn background(&self, theme: &Theme) -> Color`
// returning the canonical surface color for this elevation tier.

/// Z-order tier for UI surfaces. Pass `.z()` to the renderer at spawn time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationIndex {
    /// Root app background. Behind everything.
    Background,
    /// Default panel surface (footer, sidebar, menu bar, panels).
    Surface,
    /// Floating surface (popovers, dropdowns, tooltips).
    ElevatedSurface,
    /// Modal dialog body.
    ModalSurface,
    /// Toast / notification stack.
    Notification,
    /// Drag-and-drop preview overlay. Above everything except cursor.
    DragOverlay,
}

impl ElevationIndex {
    /// Concrete z-coordinate for this tier. Gaps allow child elements to
    /// stack within the same tier without colliding with the next one.
    #[inline]
    pub const fn z(&self) -> f32 {
        match self {
            ElevationIndex::Background      => 0.0,
            ElevationIndex::Surface         => 10.0,
            ElevationIndex::ElevatedSurface => 100.0,
            ElevationIndex::ModalSurface    => 300.0,
            ElevationIndex::Notification    => 500.0,
            ElevationIndex::DragOverlay     => 700.0,
        }
    }
}
