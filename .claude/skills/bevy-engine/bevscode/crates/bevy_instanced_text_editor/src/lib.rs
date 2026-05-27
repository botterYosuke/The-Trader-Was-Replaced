#![allow(clippy::type_complexity)]

//! Rope-backed text editor on top of [`bevy_instanced_text`].
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_instanced_text::prelude::*;
//! use bevy_instanced_text_editor::{InstancedTextEditPlugin, RopeBuffer, TextEditor};
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(InstancedTextPlugins)
//!     .add_plugins(InstancedTextEditPlugin::default())
//!     .add_systems(Startup, |mut commands: Commands| {
//!         commands.spawn((
//!             TextEditor,
//!             TextBuffer::<RopeBuffer>::new(RopeBuffer::new("edit me")),
//!             TextFont::default(),
//!         ));
//!     })
//!     .run();
//! ```

pub mod cursor_movement;
pub mod editing;
pub mod history;
pub mod line_index;
pub mod plugin;
pub mod text;
pub mod text_edit;
pub mod text_state;
pub mod typing;
pub mod widget;

pub use cursor_movement::*;
pub use editing::{point_at_byte, EditOutcome};
pub use history::{EditHistory, EditKind, EditOperation, EditTransaction};
pub use line_index::{shift_line, LineShift};
pub use plugin::{EditApplySet, EditEmitSet, InstancedTextEditPlugin};
pub use text::RopeBuffer;
pub use text_edit::*;
pub use text_state::{
    is_auto_pair_neighbor, EditDelta, EditHistoryState, EditPoint, IndentConfig, OnEdit,
    SnapshotPreEdit, TextEditor,
};

#[cfg(feature = "arboard")]
pub use bevy_instanced_text_interaction::SystemClipboard;
pub use bevy_instanced_text_interaction::{
    caret_overlay, copy_selection, cursor_blink_visible, screen_to_char_pos, selection_text,
    Anchor, AnchorBias, AnchorSet, BlinkPhase, ClipboardProvider, ClipboardResource,
    CursorBlinkingMode, CursorSettings, CursorState, CursorStyle, InstancedTextInteractionPlugin,
    InteractionSettings, KeyRepeatSettings, KeyRepeatState, NullClipboard, ScrollConfig,
    ScrollbarConfig, ScrollbarVisibility, Selection, SelectionCollection, SelectionMode,
    SelectionState, SmoothCaretAnimation, SurroundingLinesStyle, TextCursorColor, TextEdit,
    TextSelectionColor, TextViewDragState, DEFAULT_SEMANTIC_ESCAPE_CHARS,
};

pub mod prelude {
    //! Common types for spawning editable text views.
    pub use crate::{
        EditDelta, EditEmitSet, EditHistoryState, EditKind, EditOperation, EditOutcome, EditPoint,
        EditTransaction, IndentConfig, InstancedTextEditPlugin, OnEdit, RopeBuffer,
        SnapshotPreEdit, TextEditor,
    };
    pub use bevy_instanced_text_interaction::prelude::*;
}
