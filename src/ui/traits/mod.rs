//! Issue #48 — Step 7: component-facing trait pyramid.
//!
//! These traits are *declarations only*. Concrete `impl` blocks for buttons,
//! inputs, lists, etc. land in #46 (component refactor). Keeping the surface
//! tiny here lets call sites (and tests) compile against a stable API while
//! the per-component implementations are still in flux.
//!
//! Naming notes:
//! * `UiSized` (not `Sized`) avoids collision with `std::marker::Sized`.
//! * `UiStyled` / `UiStyledExt` mirror that convention for symmetry.
//!
//! Bevy version note:
//! `Clickable::on_click` takes a closure (not a Bevy `Event` / `Message`) so
//! the trait stays decoupled from the 0.15 → 0.18 `Event → Message` rename.
//! Concrete components translate the closure to a `Commands`-driven send.

use crate::ui::theme::ElevationIndex;
use bevy::prelude::Component;

// -- ComponentSize ----------------------------------------------------------

/// Discrete size token for interactive components.
///
/// Issue #46: derives `Component` so a button can carry its size token (the
/// builder inserts it and queries `&ComponentSize`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComponentSize {
    XSmall,
    Small,
    #[default]
    Default,
    Large,
}

// -- ComponentStyle ---------------------------------------------------------

/// Visual style variant for interactive components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComponentStyle {
    #[default]
    Filled,
    Outlined,
    Ghost,
    Subtle,
}

// -- Clickable --------------------------------------------------------------

/// Component can be clicked, forwarding the click to a closure.
///
/// The closure form (instead of `Event` / `Message`) keeps this trait stable
/// across the Bevy 0.15 → 0.18 rename. Component impls in #46 will translate
/// the closure into a `Commands`-driven message send.
pub trait Clickable {
    fn on_click<F>(self, on_click: F) -> Self
    where
        F: FnMut() + Send + Sync + 'static;
}

// -- Disableable ------------------------------------------------------------

/// Component can be marked disabled (non-interactive, muted styling).
pub trait Disableable {
    fn disabled(self, disabled: bool) -> Self;
}

// -- Toggleable -------------------------------------------------------------

/// Component carries a binary selected / pressed state.
pub trait Toggleable {
    fn toggle_state(self, selected: bool) -> Self;
}

// -- UiSized ----------------------------------------------------------------

/// Component accepts a discrete size token.
pub trait UiSized {
    fn size(self, size: ComponentSize) -> Self;
}

// -- UiStyled ---------------------------------------------------------------

/// Component accepts a visual-style variant.
pub trait UiStyled {
    fn style<S: Into<ComponentStyle>>(self, style: S) -> Self;
}

// -- UiStyledExt ------------------------------------------------------------

/// Extension surface for styled components: elevation + tooltip.
pub trait UiStyledExt {
    fn elevation(self, elevation: ElevationIndex) -> Self;
    fn tooltip(self, tooltip: impl Into<String>) -> Self;
}
