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

use bevy::ecs::system::Commands;
use bevy::prelude::Component;

use crate::ui::theme::ElevationIndex;

// -- ComponentSize ----------------------------------------------------------

/// Discrete size token for interactive components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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

/// Stores a click handler closure that can dispatch via `Commands`.
///
/// Attached by `Clickable::on_click`. A generic click-dispatch system
/// (introduced alongside #46 component impls) queries `(Interaction, &mut OnClick)`
/// and invokes the closure with the world's `Commands` when the element
/// transitions to `Interaction::Pressed`, letting handlers `send_event` /
/// `MessageWriter::write` / spawn / despawn freely.
#[derive(Component)]
pub struct OnClick(pub Box<dyn FnMut(&mut Commands) + Send + Sync>);

/// Component can be clicked, forwarding the click to a `Commands`-aware closure.
///
/// The closure receives `&mut Commands`, so handlers can write messages,
/// spawn / despawn entities, or queue any `Command` directly. Implementors
/// attach the closure as an [`OnClick`] component on the produced entity.
pub trait Clickable: Sized {
    fn on_click<F>(self, on_click: F) -> Self
    where
        F: FnMut(&mut Commands) + Send + Sync + 'static;
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
