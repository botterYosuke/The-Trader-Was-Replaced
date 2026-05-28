//! Per-editor settings Components for the code editor.
//!
//! All settings are `Component`s cascaded onto every `CodeEditor` entity
//! via `#[require]`, so each editor in a multi-editor app carries its own
//! independent copy. Override any subset at spawn time:
//!
//! ```rust,ignore
//! commands.spawn((
//!     CodeEditor,
//!     EditorUi { line_numbers: LineNumbers::Off, ..default() },
//!     Indentation { insert_spaces: false, tab_size: 2, ..default() },
//! ));
//! ```
//!
//! Or mutate at runtime via `Query<&mut EditorUi, With<CodeEditor>>`.
//!
//! Field names follow Monaco's `IEditorOptions` for parity.

mod auto_edit;
mod cursor;
mod find;
mod folding;
mod guides;
mod minimap;
mod misc;
mod padding;
mod performance;
mod render_settings;
mod rulers;
mod scroll;
mod selection_config;
mod sticky_scroll;
mod syntax;
mod theme;
mod ui;
mod views;
mod wrapping;

#[cfg(feature = "lsp")]
mod lsp;
#[cfg(feature = "lsp")]
mod suggest;

pub use auto_edit::*;
pub use cursor::*;
pub use find::*;
pub use folding::*;
pub use guides::*;
pub use minimap::*;
pub use misc::*;
pub use padding::*;
pub use performance::*;
pub use render_settings::*;
pub use rulers::*;
pub use scroll::*;
pub use selection_config::*;
pub use sticky_scroll::*;
pub use syntax::*;
pub use theme::*;
pub use ui::*;
pub use views::*;
pub use wrapping::*;

pub use bevy_instanced_text_editor::KeyRepeatSettings;

#[cfg(feature = "lsp")]
pub use lsp::*;
#[cfg(feature = "lsp")]
pub use suggest::*;
