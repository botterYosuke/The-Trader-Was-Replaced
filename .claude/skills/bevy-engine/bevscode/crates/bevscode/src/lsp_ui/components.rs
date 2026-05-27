//! LSP UI marker components
//!
//! These components contain all data needed to render LSP UI elements.
//! The crate's sync systems create / update marker entities with `*PopupData`
//! components; hosts query them and render however they prefer (see
//! `examples/lsp.rs` for an `egui` + `armas` reference renderer).
//!
//! # Architecture
//!
//! 1. **State resources** (e.g., `CompletionState`) hold the raw LSP data
//! 2. **Sync systems** create/update marker entities with `*PopupData` components
//! 3. **Host renderers** query marker components and draw them however they want

use bevy::prelude::*;

use super::completion::UnifiedCompletionItem;
use crate::lsp_ui_tempera::LspPopupRoot;

/// Completion popup data. Hosts query this and render however they prefer.
///
/// Anchor is given as `(line, character)` of the cursor; the renderer
/// composes screen position from those + the editor's `RowMetrics`, so
/// scroll / viewport / font changes don't need to invalidate the data.
///
/// `#[require]`s [`Node`] + [`LspPopupRoot`], so the built-in
/// [`LspUiTemperaPlugin`](crate::lsp_ui_tempera::LspUiTemperaPlugin)
/// renders this without the host wiring up extra components. The popup
/// entity is reparented under `editor` by the `on_add: LspPopupRoot`
/// observer.
#[derive(Component, Clone, Debug)]
#[require(Node, LspPopupRoot)]
pub struct CompletionPopupData {
    /// Owning editor entity. Used by renderers to look up theme, font,
    /// anchor, etc.
    pub editor: Entity,
    /// 0-indexed buffer line of the cursor (anchor row).
    pub line: u32,
    /// 0-indexed character column of the cursor (anchor column).
    pub character: u32,
    pub items: Vec<CompletionItemData>,
    pub selected_index: usize,
    /// First visible item index.
    pub scroll_offset: usize,
    pub max_visible: usize,
    pub width: f32,
    pub height: f32,
    /// Docs for the selected item. Markdown is passed as raw `value` per LSP spec.
    pub selected_documentation: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CompletionItemData {
    pub label: String,
    pub detail: Option<String>,
    pub kind_icon: String,
    pub is_word: bool,
    pub insert_text: String,
}

impl From<&UnifiedCompletionItem> for CompletionItemData {
    fn from(item: &UnifiedCompletionItem) -> Self {
        Self {
            label: item.label().to_string(),
            detail: item.detail().map(|s| s.to_string()),
            kind_icon: item.kind_icon().to_string(),
            is_word: item.is_word(),
            insert_text: item.insert_text().to_string(),
        }
    }
}

/// Hover popup data. Hosts query this and render however they prefer.
///
/// Anchor is given as `(line, character)` of the trigger position; the
/// renderer composes screen position from those + the editor's
/// `RowMetrics`.
#[derive(Component, Clone, Debug)]
#[require(Node, LspPopupRoot)]
pub struct HoverPopupData {
    pub editor: Entity,
    /// 0-indexed buffer line of the trigger position.
    pub line: u32,
    /// 0-indexed character column of the trigger position.
    pub character: u32,
    pub content: String,
    pub width: f32,
    pub height: f32,
}

/// Marker component for the signature help popup entity.
/// Contains all data needed to render signature help.
///
/// Anchor is given as `(line, character)` of the cursor; the renderer
/// composes screen position from those + the editor's `RowMetrics` and
/// flips above/below the line as needed.
#[derive(Component, Clone, Debug)]
#[require(Node, LspPopupRoot)]
pub struct SignatureHelpPopupData {
    pub editor: Entity,
    /// 0-indexed buffer line of the cursor.
    pub line: u32,
    /// 0-indexed character column of the cursor.
    pub character: u32,
    /// Signature label text
    pub label: String,
    /// Active parameter index (for highlighting)
    pub active_parameter: usize,
    /// Parameter ranges in the label (start, end) for highlighting
    pub parameter_ranges: Vec<(usize, usize)>,
    /// Total number of signatures (for "1/3" indicator)
    pub total_signatures: usize,
    /// Current signature index
    pub current_index: usize,
    /// Calculated popup width
    pub width: f32,
    /// Calculated popup height
    pub height: f32,
}

/// Code actions popup data. Hosts query this and render however they prefer.
///
/// Anchor is given as `(line, character)` of the cursor; the renderer
/// composes screen position from those + the editor's `RowMetrics`.
#[derive(Component, Clone, Debug)]
#[require(Node, LspPopupRoot)]
pub struct CodeActionsPopupData {
    pub editor: Entity,
    /// 0-indexed buffer line of the cursor.
    pub line: u32,
    /// 0-indexed character column of the cursor.
    pub character: u32,
    pub actions: Vec<CodeActionItemData>,
    pub selected_index: usize,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub struct CodeActionItemData {
    pub title: String,
    pub icon: String,
    pub is_preferred: bool,
}

/// Marker component for the rename input entity.
/// Contains all data needed to render the inline rename dialog.
///
/// Anchor is given as `(line, character)` at the symbol's start; the
/// renderer composes screen position from those + the editor's
/// `RowMetrics`.
#[derive(Component, Clone, Debug)]
#[require(Node, LspPopupRoot)]
pub struct RenameInputData {
    pub editor: Entity,
    /// 0-indexed buffer line of the symbol start.
    pub line: u32,
    /// 0-indexed character column of the symbol start.
    pub character: u32,
    /// Current input text
    pub text: String,
    /// Original symbol text (for placeholder/comparison)
    pub original_text: String,
    /// Cursor position within the text
    pub cursor_position: usize,
    /// Calculated input width
    pub width: f32,
    /// Calculated input height
    pub height: f32,
}

/// Semantic data for a single inlay hint.
///
/// Carries *what* to render (line + character + label + kind), not
/// *where*. Renderers compose the world position from these fields
/// plus the editor's `RowMetrics`.
#[derive(Component, Clone, Debug)]
pub struct InlayHintData {
    /// Hint label text.
    pub label: String,
    /// Hint kind for coloring.
    pub kind: InlayHintKind,
    /// 0-indexed buffer line.
    pub line: u32,
    /// 0-indexed character column (UTF-16 code units, per LSP).
    pub character: u32,
}

/// Kind of inlay hint (for styling)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintKind {
    /// Type annotation hint
    Type,
    /// Parameter name hint
    Parameter,
    /// Other/unknown hint
    Other,
}

/// Semantic data for a single document highlight.
///
/// Carries *what* to highlight (line + character range + read/write
/// kind), not *where* to draw it on the screen. Renderers compose the
/// world position from these fields plus the editor's
/// `RowMetrics` — that way the highlight stays anchored correctly as
/// the user scrolls, resizes, or zooms, without `sync_document_highlights`
/// re-running on every viewport change.
#[derive(Component, Clone, Debug)]
pub struct DocumentHighlightData {
    /// 0-indexed buffer line.
    pub line: u32,
    /// 0-indexed start column in characters (UTF-16 code units, per
    /// LSP).
    pub start_character: u32,
    /// 0-indexed end column in characters (exclusive). May span past
    /// the end of the line; renderers should clamp.
    pub end_character: u32,
    /// `true` for write references (assignment / definition site),
    /// `false` for read references. Renderers typically distinguish
    /// these with different background colors.
    pub is_write: bool,
}

/// Marker for entities that are part of the LSP UI.
/// Used for cleanup and querying all LSP UI entities.
#[derive(Component, Clone, Copy, Debug)]
pub struct LspUiElement;

/// Marker for the visual/rendered part of an LSP UI element.
/// The data component (e.g., `CompletionPopupData`) is on the parent entity,
/// and this marker is on the spawned visual children.
#[derive(Component, Clone, Copy, Debug)]
pub struct LspUiVisual;
