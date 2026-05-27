//! Editor-only state components: edit history, indentation, the [`TextEditor`]
//! marker, and the per-edit byte snapshot.

use bevy::prelude::*;
use ropey::Rope;

use crate::history::EditHistory;
use bevy_instanced_text_interaction::text_edit::AnchorSet;

/// Opt-in marker: clone the rope before each edit for LSP incremental sync.
/// Ropey clones are O(log n) due to structural sharing.
#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct SnapshotPreEdit;

#[derive(Component)]
pub struct EditHistoryState {
    pub history: EditHistory,
    pub anchors: AnchorSet,
    #[doc(hidden)]
    pub pending_byte_edit: Option<EditDelta>,
    #[doc(hidden)]
    pub snapshot_pre_edits: bool,
    #[doc(hidden)]
    pub pre_edit_rope: Option<Rope>,
}

impl Default for EditHistoryState {
    fn default() -> Self {
        Self {
            history: EditHistory::default(),
            anchors: AnchorSet::new(),
            pending_byte_edit: None,
            snapshot_pre_edits: false,
            pre_edit_rope: None,
        }
    }
}

/// True when `pos - 1` and `pos` form a bracket/quote auto-pair.
pub fn is_auto_pair_neighbor(rope: &Rope, pos: usize) -> bool {
    if pos == 0 || pos >= rope.len_chars() {
        return false;
    }
    let opener = rope.char(pos - 1);
    let closer = rope.char(pos);
    matches!(
        (opener, closer),
        ('(', ')') | ('[', ']') | ('{', '}') | ('<', '>') | ('"', '"') | ('\'', '\'') | ('`', '`')
    )
}

/// Emitted per edit. `pre_edit_rope` is `Some` only with [`SnapshotPreEdit`].
#[derive(Message, EntityEvent, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct OnEdit {
    pub entity: Entity,
    pub byte_edit: Option<EditDelta>,
    #[reflect(ignore)]
    pub pre_edit_rope: Option<Rope>,
}

/// 0-indexed `(row, byte_column)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub struct EditPoint {
    pub row: u32,
    pub column_byte: u32,
}

#[derive(Clone, Copy, Debug, Reflect)]
pub struct EditDelta {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_position: EditPoint,
    pub old_end_position: EditPoint,
    pub new_end_position: EditPoint,
}

#[derive(Component, Clone, Copy, Debug, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct IndentConfig {
    pub tab_width: usize,
    /// `false` inserts a literal `\t`.
    pub use_spaces: bool,
    /// Spaces inserted on Tab snap to the next multiple of `tab_width`.
    pub use_tab_stops: bool,
    /// Backspace inside leading whitespace deletes back to the previous tab stop.
    pub sticky_tab_stops: bool,
    /// Backspace at the end of a run of trailing whitespace deletes the whole run.
    pub trim_whitespace_on_delete: bool,
}

impl Default for IndentConfig {
    fn default() -> Self {
        Self {
            tab_width: 4,
            use_spaces: true,
            use_tab_stops: true,
            sticky_tab_stops: false,
            trim_whitespace_on_delete: false,
        }
    }
}

/// Spawning just `TextEditor` is enough for a fully-rendered editor;
/// `#[require]` cascades all supporting state.
#[derive(Component, Default, Reflect)]
#[reflect(Component, Default)]
#[require(
    bevy_instanced_text::TextBuffer<crate::text::RopeBuffer>,
    bevy_instanced_text_interaction::CursorState,
    bevy_instanced_text_interaction::SelectionState,
    EditHistoryState,
    IndentConfig,
    bevy_instanced_text_interaction::TextViewDragState,
    bevy_instanced_text_interaction::ScrollConfig,
    bevy_instanced_text_interaction::CursorSettings,
    bevy_instanced_text_interaction::BlinkPhase,
    bevy_instanced_text_interaction::InteractionSettings,
)]
pub struct TextEditor;
