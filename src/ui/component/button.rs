//! Button component — `ButtonStyle × ButtonState` color table (Issue #46 Slice A).
//!
//! Modeled on Zed's `crates/ui/src/components/button/button_like.rs`: button
//! colors are resolved by a single pure function [`button_colors`] keyed on a
//! `(ButtonStyle, ButtonState)` pair, against the active [`Theme`]. A single
//! generic `button_interaction_system` (added later in this slice) reads the
//! table for every button, replacing the ~13 scattered per-button color
//! systems.

use crate::ui::theme::{ElevationIndex, Theme};
use bevy::prelude::*;

// -- ButtonStyle ------------------------------------------------------------

/// Visual style variant of a button. The first axis of the color table.
///
/// Mirrors Zed's `ButtonStyle`. `Tinted` carries a [`TintColor`] so the four
/// semantic accents (Accent / Error / Warning / Success) share one arm.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonStyle {
    /// Emphasis fill (Run / Submit): solid neutral element background.
    Filled,
    /// Semantic accent fill (Accent / Error / Warning / Success).
    Tinted(TintColor),
    /// Transparent body with a visible border.
    Outlined,
    /// Outlined, but the body uses the softer ghost element states.
    OutlinedGhost,
    /// Default low-emphasis button: ghost background, hover reveals.
    Subtle,
    /// No background; only the foreground (label/icon) color changes.
    Transparent,
}

/// Semantic tint applied to [`ButtonStyle::Tinted`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TintColor {
    Accent,
    Error,
    Warning,
    Success,
}

// -- ButtonState ------------------------------------------------------------

/// Interaction / selection state of a button. The second axis of the table.
///
/// Derived from Bevy `Interaction` plus the `ButtonSelected` / `ButtonDisabled`
/// marker components by the generic interaction system. `Focused` is reserved
/// for a later focus-ring slice and is not produced in Slice A.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonState {
    Enabled,
    Hovered,
    Active,
    Focused,
    Disabled,
    Selected,
}

// -- ButtonColors -----------------------------------------------------------

/// Resolved colors for one `(style, state)` cell.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ButtonColors {
    pub background: Color,
    pub border: Color,
    pub label: Color,
    pub icon: Color,
}

// -- button_colors ----------------------------------------------------------

/// The four tint tokens for a [`TintColor`]: solid fill, soft background,
/// border, and the on-soft-background label color.
fn tint_tokens(tint: TintColor, theme: &Theme) -> (Color, Color, Color, Color) {
    let c = &theme.colors;
    let s = &theme.status;
    match tint {
        // Accent uses the neutral/accent palette rather than StatusColors.
        TintColor::Accent => (c.accent, c.element_selection_background, c.border_selected, c.text_accent),
        TintColor::Error => (s.error, s.error_background, s.error_border, s.error),
        TintColor::Warning => (s.warning, s.warning_background, s.warning_border, s.warning),
        TintColor::Success => (s.success, s.success_background, s.success_border, s.success),
    }
}

/// Resolve the [`ButtonColors`] for a `(style, state)` pair against `theme`.
///
/// This is the single source of truth for the `ButtonStyle × ButtonState`
/// table (Zed's `button_like.rs` analogue). The generic
/// `button_interaction_system` calls this for every themed button.
///
/// `elevation` is reserved for the "fall through to the surface behind"
/// styles (`Transparent` / `Disabled` bodies, focus overlays); it is not yet
/// read by the table and is kept in the signature per the #46 API so later
/// slices need not change call sites.
pub fn button_colors(
    style: ButtonStyle,
    state: ButtonState,
    _elevation: ElevationIndex,
    theme: &Theme,
) -> ButtonColors {
    let c = &theme.colors;
    let none = Color::NONE;

    // Resolve (background, border, label) for this cell. `icon` is derived
    // from `label` below so the foreground tiers stay in lockstep.
    let (background, border, label) = match (style, state) {
        // -- Filled --------------------------------------------------------
        (ButtonStyle::Filled, ButtonState::Enabled) => (c.element_background, none, c.text),
        (ButtonStyle::Filled, ButtonState::Hovered) => (c.element_hover, none, c.text),
        (ButtonStyle::Filled, ButtonState::Active) => (c.element_active, none, c.text),
        (ButtonStyle::Filled, ButtonState::Selected) => (c.element_selected, c.border_selected, c.text),
        (ButtonStyle::Filled, ButtonState::Focused) => (c.element_background, c.border_focused, c.text),
        (ButtonStyle::Filled, ButtonState::Disabled) => (c.element_disabled, c.border_disabled, c.text_disabled),

        // -- Tinted(t) -----------------------------------------------------
        (ButtonStyle::Tinted(t), state) => {
            let (solid, soft_bg, t_border, t_label) = tint_tokens(t, theme);
            match state {
                ButtonState::Enabled => (soft_bg, t_border, t_label),
                ButtonState::Hovered | ButtonState::Active | ButtonState::Selected => {
                    (solid, t_border, c.text)
                }
                ButtonState::Focused => (soft_bg, c.border_focused, t_label),
                ButtonState::Disabled => (c.element_disabled, c.border_disabled, c.text_disabled),
            }
        }

        // -- Outlined ------------------------------------------------------
        (ButtonStyle::Outlined, ButtonState::Enabled) => (none, c.border, c.text),
        (ButtonStyle::Outlined, ButtonState::Hovered) => (c.element_hover, c.border_variant, c.text),
        (ButtonStyle::Outlined, ButtonState::Active) => (c.element_active, c.border_variant, c.text),
        (ButtonStyle::Outlined, ButtonState::Selected) => (c.element_selected, c.border_selected, c.text),
        (ButtonStyle::Outlined, ButtonState::Focused) => (none, c.border_focused, c.text),
        (ButtonStyle::Outlined, ButtonState::Disabled) => (none, c.border_disabled, c.text_disabled),

        // -- OutlinedGhost -------------------------------------------------
        (ButtonStyle::OutlinedGhost, ButtonState::Enabled) => (none, c.border, c.text_muted),
        (ButtonStyle::OutlinedGhost, ButtonState::Hovered) => (c.ghost_element_hover, c.border, c.text),
        (ButtonStyle::OutlinedGhost, ButtonState::Active) => (c.ghost_element_active, c.border, c.text),
        (ButtonStyle::OutlinedGhost, ButtonState::Selected) => (c.ghost_element_selected, c.border_selected, c.text),
        (ButtonStyle::OutlinedGhost, ButtonState::Focused) => (none, c.border_focused, c.text_muted),
        (ButtonStyle::OutlinedGhost, ButtonState::Disabled) => (none, c.border_disabled, c.text_disabled),

        // -- Subtle --------------------------------------------------------
        (ButtonStyle::Subtle, ButtonState::Enabled) => (c.ghost_element_background, none, c.text_muted),
        (ButtonStyle::Subtle, ButtonState::Hovered) => (c.ghost_element_hover, none, c.text),
        (ButtonStyle::Subtle, ButtonState::Active) => (c.ghost_element_active, none, c.text),
        (ButtonStyle::Subtle, ButtonState::Selected) => (c.ghost_element_selected, none, c.text),
        (ButtonStyle::Subtle, ButtonState::Focused) => (c.ghost_element_background, c.border_focused, c.text_muted),
        (ButtonStyle::Subtle, ButtonState::Disabled) => (c.ghost_element_disabled, none, c.text_disabled),

        // -- Transparent ---------------------------------------------------
        (ButtonStyle::Transparent, ButtonState::Enabled) => (none, none, c.text_muted),
        (ButtonStyle::Transparent, ButtonState::Hovered) => (c.ghost_element_hover, none, c.text),
        (ButtonStyle::Transparent, ButtonState::Active) => (c.ghost_element_active, none, c.text),
        (ButtonStyle::Transparent, ButtonState::Selected) => (c.ghost_element_selected, none, c.text),
        (ButtonStyle::Transparent, ButtonState::Focused) => (none, c.border_focused, c.text_muted),
        (ButtonStyle::Transparent, ButtonState::Disabled) => (none, none, c.text_disabled),
    };

    // Mirror the label tier onto the icon tier.
    let icon = if label == c.text_disabled {
        c.icon_disabled
    } else if label == c.text_muted {
        c.icon_muted
    } else if label == c.text_accent {
        c.icon_accent
    } else {
        c.icon
    };

    ButtonColors { background, border, label, icon }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bc(style: ButtonStyle, state: ButtonState) -> ButtonColors {
        button_colors(style, state, ElevationIndex::Surface, &Theme::default())
    }

    /// Filled column: each interaction state maps to its element tier.
    #[test]
    fn filled_states_map_to_element_tiers() {
        let t = Theme::default();
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Enabled).background, t.colors.element_background);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Hovered).background, t.colors.element_hover);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Active).background, t.colors.element_active);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Selected).background, t.colors.element_selected);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Disabled).background, t.colors.element_disabled);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Disabled).label, t.colors.text_disabled);
    }

    /// Tinted resolves to StatusColors for semantic tints and accent tokens
    /// for Accent. Enabled shows the soft background; Hovered the solid fill.
    #[test]
    fn tinted_uses_status_and_accent_tokens() {
        let t = Theme::default();
        // Success: soft bg when enabled, solid fill on hover.
        assert_eq!(bc(ButtonStyle::Tinted(TintColor::Success), ButtonState::Enabled).background, t.status.success_background);
        assert_eq!(bc(ButtonStyle::Tinted(TintColor::Success), ButtonState::Hovered).background, t.status.success);
        // Error border + label.
        assert_eq!(bc(ButtonStyle::Tinted(TintColor::Error), ButtonState::Enabled).border, t.status.error_border);
        assert_eq!(bc(ButtonStyle::Tinted(TintColor::Error), ButtonState::Enabled).label, t.status.error);
        // Accent enabled label is the accent text token (drives icon_accent).
        let accent = bc(ButtonStyle::Tinted(TintColor::Accent), ButtonState::Enabled);
        assert_eq!(accent.label, t.colors.text_accent);
        assert_eq!(accent.icon, t.colors.icon_accent);
    }

    /// Outlined: transparent body + visible border when enabled.
    #[test]
    fn outlined_enabled_is_transparent_with_border() {
        let t = Theme::default();
        let o = bc(ButtonStyle::Outlined, ButtonState::Enabled);
        assert_eq!(o.background, Color::NONE);
        assert_eq!(o.border, t.colors.border);
    }

    /// OutlinedGhost enabled uses a muted label; hover reveals ghost bg.
    #[test]
    fn outlined_ghost_enabled_is_muted() {
        let t = Theme::default();
        assert_eq!(bc(ButtonStyle::OutlinedGhost, ButtonState::Enabled).label, t.colors.text_muted);
        assert_eq!(bc(ButtonStyle::OutlinedGhost, ButtonState::Hovered).background, t.colors.ghost_element_hover);
    }

    /// Subtle: ghost background by default, hover/active climb the ghost tiers.
    #[test]
    fn subtle_uses_ghost_tiers() {
        let t = Theme::default();
        assert_eq!(bc(ButtonStyle::Subtle, ButtonState::Enabled).background, t.colors.ghost_element_background);
        assert_eq!(bc(ButtonStyle::Subtle, ButtonState::Hovered).background, t.colors.ghost_element_hover);
        assert_eq!(bc(ButtonStyle::Subtle, ButtonState::Enabled).label, t.colors.text_muted);
    }

    /// Transparent: no background at rest, foreground-only.
    #[test]
    fn transparent_enabled_has_no_background() {
        let t = Theme::default();
        let tr = bc(ButtonStyle::Transparent, ButtonState::Enabled);
        assert_eq!(tr.background, Color::NONE);
        assert_eq!(tr.border, Color::NONE);
        assert_eq!(tr.label, t.colors.text_muted);
    }

    /// icon tier mirrors the label tier (muted/disabled/accent/default).
    #[test]
    fn icon_mirrors_label_tier() {
        let t = Theme::default();
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Disabled).icon, t.colors.icon_disabled);
        assert_eq!(bc(ButtonStyle::Subtle, ButtonState::Enabled).icon, t.colors.icon_muted);
        assert_eq!(bc(ButtonStyle::Filled, ButtonState::Enabled).icon, t.colors.icon);
    }
}
