//! Z-order elevation tokens.
//!
//! Modeled after Zed's `crates/ui/src/styles/elevation.rs`. Every surface
//! that participates in z-ordering should pick an `ElevationIndex` rather
//! than writing a raw `z` literal. Gaps between tiers (10 → 100 → 300 …)
//! leave room for child z-offsets inside the same elevation tier.
//!
//! Issue #46: derives `Component` so a UI surface can carry its tier directly
//! (themed buttons query `&ElevationIndex`; the component layer reuses it).

use bevy::prelude::Component;

/// Z-order tier for UI surfaces. Pass `.z()` to the renderer at spawn time.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    /// Canonical surface color for this elevation tier, resolved against the
    /// active `Theme`. Pair with `.z()` when spawning a surface so color and
    /// stacking stay in sync.
    #[inline]
    pub fn background(&self, theme: &crate::ui::theme::Theme) -> bevy::color::Color {
        self.background_for_colors(&theme.colors)
    }

    /// Lower-level form of [`background`] that takes a `ThemeColors` directly.
    /// Used by `ElevationTokens::background` so the tier → color match table
    /// lives in exactly one place.
    pub fn background_for_colors(
        &self,
        colors: &crate::ui::theme::ThemeColors,
    ) -> bevy::color::Color {
        match self {
            ElevationIndex::Background      => colors.background,
            ElevationIndex::Surface         => colors.surface_background,
            ElevationIndex::ElevatedSurface => colors.elevated_surface_background,
            ElevationIndex::ModalSurface    => colors.modal_background,
            ElevationIndex::Notification    => colors.notification_background,
            ElevationIndex::DragOverlay     => colors.drag_overlay_background,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    /// RED until `ElevationIndex::background` is implemented in Slice 1 GREEN.
    /// We assert the tier → ThemeColors mapping from plan §Step 2.
    #[test]
    fn background_returns_theme_colors_for_each_tier() {
        let theme = Theme::default();

        assert_eq!(
            ElevationIndex::Background.background(&theme),
            theme.colors.background,
            "Background tier should map to colors.background"
        );
        assert_eq!(
            ElevationIndex::Surface.background(&theme),
            theme.colors.surface_background,
            "Surface tier should map to colors.surface_background"
        );
        assert_eq!(
            ElevationIndex::ElevatedSurface.background(&theme),
            theme.colors.elevated_surface_background,
            "ElevatedSurface tier should map to colors.elevated_surface_background"
        );
        assert_eq!(
            ElevationIndex::ModalSurface.background(&theme),
            theme.colors.modal_background,
            "ModalSurface tier should map to colors.modal_background"
        );
        assert_eq!(
            ElevationIndex::Notification.background(&theme),
            theme.colors.notification_background,
            "Notification tier should map to colors.notification_background"
        );
        assert_eq!(
            ElevationIndex::DragOverlay.background(&theme),
            theme.colors.drag_overlay_background,
            "DragOverlay tier should map to colors.drag_overlay_background"
        );
    }
}
