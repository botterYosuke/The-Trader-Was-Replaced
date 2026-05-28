//! Core types for the code editor.

pub mod display_map;
pub mod editor;
pub mod events;
pub mod fold;

pub use bevy_instanced_text_editor::{
    Anchor, AnchorBias, AnchorSet, CursorState, EditHistory, EditHistoryState, EditKind,
    EditOperation, EditTransaction, IndentConfig, Selection, SelectionCollection, SelectionState,
    TextEdit, TextEditor,
};

pub use display_map::*;
pub use editor::*;
pub use fold::*;
