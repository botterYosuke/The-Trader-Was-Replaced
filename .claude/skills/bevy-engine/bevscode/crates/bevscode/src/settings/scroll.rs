//! Scroll settings re-exported from `bevy_instanced_text_interaction` so
//! `crate::settings::*` is the single import point for Monaco-parity config.
//!
//! `ScrollConfig` covers Monaco's `mouseWheelScrollSensitivity`,
//! `smoothScrolling`, `scrollBeyondLastLine`/`Column`, `mouseWheelZoom`,
//! `fastScrollSensitivity`, `scrollPredominantAxis`,
//! `revealHorizontalRightPadding`, and the nested `scrollbar.*` knobs.

pub use bevy_instanced_text_editor::{ScrollConfig, ScrollbarConfig, ScrollbarVisibility};
