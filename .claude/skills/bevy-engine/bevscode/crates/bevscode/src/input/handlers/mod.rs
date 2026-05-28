//! Per-action handler systems for IDE-specific actions (multi-cursor,
//! folding, goto-line, LSP requests).

pub mod file;
pub mod folding;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod multi_cursor;

#[cfg(feature = "lsp")]
pub mod lsp_followup;

/// Helper: resolve the focused editor entity. Returns `None` when the
/// editor isn't focused or when the focused entity isn't a `CodeEditor`.
///
/// All handler systems early-return on `None`; they never act on a
/// non-focused editor.
#[macro_export]
macro_rules! editor_focused_entity {
    ($input_focus:expr) => {
        match $input_focus.get() {
            Some(e) => e,
            None => return,
        }
    };
}
