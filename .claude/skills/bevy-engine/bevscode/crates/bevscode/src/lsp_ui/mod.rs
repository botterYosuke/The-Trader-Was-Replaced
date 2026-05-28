//! Editor-coupled LSP UI adapter.
//!
//! The transport layer (JSON-RPC client, server capabilities, document URI /
//! version) lives in the peer crate [`bevy_lsp`] as per-entity Components. This
//! module is the editor-coupled adapter on top:
//!
//! - [`state`] — popup-state Components (completion popup, hover, signature
//!   help, code actions, rename, inlay hints, document highlights), plus
//!   debounce timers and the editor's `did_change` driver state. All Components
//!   on the editor entity, all populated by the editor's `#[require]` cascade.
//! - [`components`] — *render-data* Components materialized by the sync
//!   systems (e.g. `CompletionPopupData`). Hosts query these and draw them
//!   however they want; see `examples/lsp.rs` for an `egui` + `armas`
//!   reference renderer.
//! - [`sync`] — systems that translate the popup-state Components into the
//!   render-data Components above.
//! - [`event_listeners`] — systems that translate editor events
//!   ([`crate::types::events::TextEdited`], request events) into LSP
//!   `LspMessage` sends through the entity's [`bevy_lsp::LspClient`].
//! - [`systems`] — `process_lsp_messages` (drain `LspResponse`s into editor
//!   state) plus a few capability-aware request fanouts (inlay hints,
//!   document highlights).
//!
//! Hosts that want completion / hover / etc. data must add this crate's
//! `plugin::LspPlugin`; the editor cascade handles the rest.

pub mod completion;
pub mod components;
pub mod event_listeners;
pub mod inlay_splice;
pub mod interceptors;
pub mod lifecycle;
pub mod snippet;
pub mod state;
pub mod sync;
pub mod systems;

pub mod prelude {
    pub use bevy_lsp::{
        CodeActionOrCommand, LspClient, LspDocument, LspMessage, LspResponse, RequestType,
        ServerCapabilities, DEFAULT_REQUEST_TIMEOUT_SECS,
    };

    pub use super::completion::{
        LspCompletionPopup, UnifiedCompletionItem, WordCompletionItem,
        COMPLETION_MAX_VISIBLE_DEFAULT,
    };

    pub use super::lifecycle::{
        CodeActionsLifecycle, CodeActionsPopupBackref, CompletionLifecycle, CompletionPopupBackref,
        HoverLifecycle, HoverPopupBackref, PopupLifecycleData, PopupObserversAttached,
        RenameLifecycle, RenamePopupBackref, SignatureLifecycle, SignaturePopupBackref,
    };

    pub use super::state::{
        LspCodeActionsPopup, LspDebounceTimers, LspDidChangeBatcher, LspDocumentHighlights,
        LspHoverPopup, LspInlayHints, LspRenamePopup, LspSignatureHelpPopup, PendingLspRequest,
    };

    pub use super::components::{
        CodeActionItemData, CodeActionsPopupData, CompletionItemData, CompletionPopupData,
        DocumentHighlightData, HoverPopupData, InlayHintData, InlayHintKind, LspUiElement,
        LspUiVisual, RenameInputData, SignatureHelpPopupData,
    };
    pub use super::event_listeners::{
        listen_apply_completion, listen_completion_requests, listen_dismiss_completion,
        listen_hover_requests, listen_rename_requests, listen_signature_help_requests,
        listen_text_edit_events,
    };
    pub use super::sync::{
        sync_code_actions_popup, sync_completion_popup, sync_document_highlights, sync_hover_popup,
        sync_inlay_hints, sync_rename_input, sync_signature_help_popup,
    };
    pub use super::systems::{
        cleanup_lsp_timeouts, execute_code_action, request_code_actions, request_inlay_hints,
        request_signature_help, sync_lsp_document, DiagnosticMarker, LocationType,
        MultipleLocationsEvent, NavigateToFileEvent,
    };
}

pub use bevy_lsp::{
    CodeActionOrCommand, LspClient, LspDocument, LspMessage, LspRequest, LspResponse, RequestType,
    ServerCapabilities,
};
pub use completion::{
    LspCompletionPopup, UnifiedCompletionItem, WordCompletionItem, COMPLETION_MAX_VISIBLE_DEFAULT,
};
pub use lifecycle::{
    CodeActionsLifecycle, CodeActionsPopupBackref, CompletionLifecycle, CompletionPopupBackref,
    HoverLifecycle, HoverPopupBackref, PopupLifecycleData, PopupObserversAttached,
    RenameLifecycle, RenamePopupBackref, SignatureLifecycle, SignaturePopupBackref,
};
pub use state::{
    LspCodeActionsPopup, LspDebounceTimers, LspDidChangeBatcher, LspDocumentHighlights,
    LspHoverPopup, LspInlayHints, LspRenamePopup, LspSignatureHelpPopup, PendingLspRequest,
};
pub use systems::{
    sync_lsp_document, DiagnosticMarker, LocationType, MultipleLocationsEvent, NavigateToFileEvent,
};

pub fn reset_hover_state(hover_state: &mut state::LspHoverPopup) {
    hover_state.reset();
}
