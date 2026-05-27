//! Editor events for inter-plugin communication.

use bevy::prelude::*;

/// Notifies plugins (syntax highlighting, LSP, etc.) about text changes for
/// incremental updates. Positions are captured at edit-time so consumers
/// don't need the pre-edit rope for tree-sitter style byte-keyed edits.
///
/// `pre_edit_rope` is `Some` when the editor entity has the
/// [`bevy_instanced_text_editor::SnapshotPreEdit`] marker (LSP attaches it). LSP
/// incremental sync needs the pre-edit rope to convert byte offsets
/// into LSP positions in the server's negotiated encoding.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct TextEdited {
    pub delta: bevy_instanced_text_editor::EditDelta,
    pub content_version: u64,
    #[reflect(ignore)]
    pub pre_edit_rope: Option<ropey::Rope>,
}

impl TextEdited {
    pub fn new(delta: bevy_instanced_text_editor::EditDelta, content_version: u64) -> Self {
        Self {
            delta,
            content_version,
            pre_edit_rope: None,
        }
    }

    pub fn with_pre_edit_rope(mut self, rope: Option<ropey::Rope>) -> Self {
        self.pre_edit_rope = rope;
        self
    }

    pub fn start_byte(&self) -> usize {
        self.delta.start_byte
    }
    pub fn old_end_byte(&self) -> usize {
        self.delta.old_end_byte
    }
    pub fn new_end_byte(&self) -> usize {
        self.delta.new_end_byte
    }
}

/// Fired when user presses Ctrl+Space or types a trigger character.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct CompletionRequested {
    pub cursor_char: usize,
}

impl CompletionRequested {
    pub fn new(cursor_char: usize) -> Self {
        Self { cursor_char }
    }
}

/// Fired when the pointer hovers over a symbol long enough to trigger a request.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct HoverRequested {
    pub cursor_char: usize,
}

impl HoverRequested {
    pub fn new(cursor_char: usize) -> Self {
        Self { cursor_char }
    }
}

/// Fired when the user initiates a rename.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct RenameRequested {
    pub cursor_char: usize,
}

impl RenameRequested {
    pub fn new(cursor_char: usize) -> Self {
        Self { cursor_char }
    }
}

/// Fired when a signature-help trigger character is typed.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct SignatureHelpRequested {
    pub cursor_char: usize,
}

impl SignatureHelpRequested {
    pub fn new(cursor_char: usize) -> Self {
        Self { cursor_char }
    }
}

/// Close the completion popup without applying any item.
#[derive(Message, Clone, Debug, Default, Reflect)]
#[reflect(Clone, Debug, Default)]
pub struct CompletionDismissed;

/// Apply the completion item at `item_index` in the popup list.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct CompletionApplied {
    pub item_index: usize,
}

impl CompletionApplied {
    pub fn new(item_index: usize) -> Self {
        Self { item_index }
    }
}

/// Inbound: swap the editor's `bevy_tree_sitter::Language` component to one
/// the host already constructed (e.g. picked from a language registry by
/// filename). Triggers a re-parse and re-highlight on the next frame.
/// `language` is `Option` so hosts can clear back to "no syntax."
///
/// Not Reflect: `bevy_tree_sitter::TreeSitterGrammar` carries `tree_sitter::Language`
/// FFI state. Gated on the `tree-sitter` feature.
#[derive(Message, Clone)]
pub struct SetLanguageRequested {
    pub entity: Entity,
    pub grammar: Option<bevy_tree_sitter::TreeSitterGrammar>,
}

/// Outbound: a fold region's `is_folded` flipped, or a `fold_all` /
/// `unfold_all` was applied. Hosts can subscribe to update gutter
/// affordances (chevrons), minimap markers, or layout caches without
/// polling `Changed<FoldState>` (which fires on any field write — including
/// content_version bumps from the detector).
///
/// `start_line` is the region's start line; `is_folded` is the new state.
/// For bulk operations (`fold_all`/`unfold_all`) an event fires per region.
#[derive(Message, Clone, Debug, Reflect)]
#[reflect(Clone, Debug)]
pub struct FoldStateChanged {
    pub entity: Entity,
    pub start_line: usize,
    pub is_folded: bool,
}
