//! Issue #46 — component layer (Slice A onward).
//!
//! Reusable, theme-driven UI components built on top of the #48 design system
//! (`crate::ui::theme` tokens + `crate::ui::traits` pyramid). Every helper takes
//! `&Theme` and never bakes raw colors; all interactive color state is resolved
//! through a single `ButtonStyle × ButtonState` table (see [`button`]).
//!
//! Slice A: [`button`] — `ButtonStyle` / `ButtonState` / `button_colors` table,
//! the single generic `button_interaction_system`, and the `spawn_button`
//! builder implementing the trait pyramid. Future siblings (IconButton /
//! ToggleButton / SplitButton, then Slice B+ Modal / Label / Input) reuse the
//! same table.

pub mod button;
pub mod keyboard_drain;
pub mod label;
pub mod modal_layer;

pub use button::{
    button_colors, button_interaction_system, ButtonColors, ButtonDisabled, ButtonSelected,
    ButtonState, ButtonStyle, TintColor,
};
pub use label::{
    spawn_divider, spawn_indicator, spawn_labeled_value_row, spawn_table_headers_at,
};

mod hit_target;
pub use hit_target::spawn_transparent_hit_sprite;
