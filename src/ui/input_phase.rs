//! Input-handling phase ordering for the UI layer (Issue #46).
//!
//! A single `Update` SystemSet that fixes the order of the four input stages
//! so keyboard drain, modal Esc handling, widget interaction, and the
//! cosmic/bevscode editor input never race. Introduced minimally in #46
//! Slice A — only [`InputPhase::WidgetInput`] has a member so far
//! (`button_interaction_system`). The remaining phases are declared now and
//! populated by #46 Slice E (`KeyboardDrain`) / Slice B (`ModalInput`) and
//! #50 (`CosmicEdit`).
//!
//! NOTE: #48 deferred this set to #50 (`theme/mod.rs` scope guard); #46 needs
//! it for the Slice A acceptance criterion, so it lands here and #50 reuses
//! it rather than redefining it. See issue #46 comment for the rationale.

use bevy::prelude::*;

/// Ordered input phases for the `Update` schedule. Configure with
/// `.chain()` so each phase strictly precedes the next.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputPhase {
    /// Raw keyboard event drain (Slice E: `drain_keyboard*`).
    KeyboardDrain,
    /// Modal layer Esc / dismiss handling (Slice B: `ModalLayer`).
    ModalInput,
    /// Widget interaction (Slice A: `button_interaction_system`).
    WidgetInput,
    /// Cosmic / bevscode editor input (#50).
    CosmicEdit,
}

impl InputPhase {
    /// Configure the canonical ordering on the `Update` schedule. Call once
    /// from `UiPlugin::build` and from every test `App` that exercises a
    /// system registered in one of these phases.
    pub fn configure(app: &mut App) {
        app.configure_sets(
            Update,
            (
                InputPhase::KeyboardDrain,
                InputPhase::ModalInput,
                InputPhase::WidgetInput,
                InputPhase::CosmicEdit,
            )
                .chain(),
        );
    }
}
