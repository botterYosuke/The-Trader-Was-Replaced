//! Per-editor LSP UI state Components.
//!
//! These were Resources living in `bevy_lsp::state` until the protocol/UI split;
//! they're popups, debounce timers, and filter state that belong to *the
//! editor's UI layer*, not the LSP protocol. Each is a Component on the editor
//! entity (the same entity that carries [`bevy_lsp::LspClient`] and
//! [`bevy_lsp::LspDocument`]).
//!
//! Consumers query them as `Query<&mut LspCompletionPopup, With<CodeEditor>>`
//! etc. The `#[require]` cascade on [`crate::types::CodeEditor`] inserts them
//! all with `Default`, so a freshly spawned `CodeEditor` is fully usable.

use bevy::prelude::*;
use bevy_instanced_text_editor::Anchor;
use lsp_types::*;

/// One tabstop in an active snippet session, anchored so it survives
/// edits that happen while the session is live (e.g. typing into the
/// placeholder selection replaces the placeholder, and subsequent
/// tabstops shift accordingly).
#[derive(Clone, Debug)]
pub struct SessionTabstop {
    pub id: u32,
    pub start: Anchor,
    pub end: Anchor,
}

/// Active snippet tabstop session. Spawned on the editor entity by
/// `listen_apply_completion` when the inserted item carries snippet
/// syntax. Ended (despawned via `Option<&mut>` clear) when the cursor
/// leaves the session, the user presses Esc, or all stops are visited.
#[derive(Component, Default, Debug)]
pub struct TabstopSession {
    /// Stops in walk order: rising `id` values, then `0` (final stop)
    /// last. Empty when no session is active.
    pub stops: Vec<SessionTabstop>,
    /// Index into `stops` of the currently-selected tabstop. The
    /// session ends when this exceeds `stops.len() - 1`.
    pub current: usize,
}

impl TabstopSession {
    pub fn is_active(&self) -> bool {
        !self.stops.is_empty() && self.current < self.stops.len()
    }

    pub fn end(&mut self) {
        self.stops.clear();
        self.current = 0;
    }
}

/// Per-editor hover popup state.
///
/// Was `bevy_lsp::HoverState` (Resource).
#[derive(Component)]
pub struct LspHoverPopup {
    /// Whether the hover box is currently visible
    pub visible: bool,
    /// Content to display in the hover box. Format is described by `kind`.
    pub content: String,
    /// Source format of `content`. UI consumers route the markdown path
    /// through a markdown renderer when `kind == Markdown`; otherwise
    /// they fall back to plain-text rendering.
    pub kind: MarkupKind,
    /// The character index in the document where the mouse currently is
    pub trigger_char_index: usize,
    /// Debounce timer for the *trigger* path (pointer-stopped-on-cell).
    /// Distinct from the dismiss-grace timer on [`HoverLifecycle`].
    pub timer: Option<Timer>,
    /// The actual LSP range for the hover content (useful for highlighting).
    /// The same range is mirrored to `HoverLifecycle::hot_zone` so the
    /// pointer-move observer can suppress re-arm inside it.
    pub range: Option<Range>,
    /// Last viewport-local pointer position seen by the hover-move observer.
    /// Used to skip the rope/layout hit-test when the pointer has barely
    /// moved since the previous event (a per-pixel `Pointer<Move>` would
    /// otherwise re-run `screen_to_char_pos` on every sub-pixel jitter).
    pub last_pointer_pos: Option<bevy::math::Vec2>,
}

impl Default for LspHoverPopup {
    fn default() -> Self {
        Self {
            visible: false,
            content: String::new(),
            // Default to plain text — servers that send markdown override
            // this on the response. Picking `PlainText` as the default
            // keeps cold-state rendering deterministic regardless of
            // whether the markdown feature is on.
            kind: MarkupKind::PlainText,
            trigger_char_index: 0,
            timer: None,
            range: None,
            last_pointer_pos: None,
        }
    }
}

impl LspHoverPopup {
    /// Clear popup-local state. The id-bump that invalidates in-flight
    /// LSP responses + clears the hot zone lives on [`HoverLifecycle`]
    /// — call its `.dismiss()` at the same site.
    pub fn reset(&mut self) {
        self.visible = false;
        self.content.clear();
        self.kind = MarkupKind::PlainText;
        self.timer = None;
        self.range = None;
    }
}

/// Per-editor signature help popup state.
///
/// Was `bevy_lsp::SignatureHelpState` (Resource).
#[derive(Component, Default)]
pub struct LspSignatureHelpPopup {
    pub visible: bool,
    pub signatures: Vec<SignatureInformation>,
    pub active_signature: usize,
    pub active_parameter: usize,
    pub trigger_position: usize,
}

impl LspSignatureHelpPopup {
    pub fn current_signature(&self) -> Option<&SignatureInformation> {
        self.signatures.get(self.active_signature)
    }

    /// Clear popup-local state. The id-bump that invalidates in-flight
    /// LSP responses lives on [`SignatureLifecycle`] — call its
    /// `.dismiss()` at the same site.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.signatures.clear();
        self.active_signature = 0;
        self.active_parameter = 0;
    }

    /// Backward-compat alias.
    pub fn reset(&mut self) {
        self.dismiss();
    }
}

/// Per-editor code actions popup state. Holds the response from
/// `textDocument/codeAction` (quick-fix / refactor menu).
///
/// **Producer not yet wired.** The transport helper and the response
/// handler are in place; a system that watches for diagnostics under
/// the cursor (or an explicit user trigger like `Ctrl+.`) still needs
/// to be added before the lightbulb UI is fed.
#[derive(Component, Default)]
pub struct LspCodeActionsPopup {
    pub visible: bool,
    pub actions: Vec<bevy_lsp::CodeActionOrCommand>,
    pub selected_index: usize,
    pub range: Option<Range>,
}

impl LspCodeActionsPopup {
    /// Clear popup-local state. The id-bump that invalidates in-flight
    /// LSP responses lives on [`CodeActionsLifecycle`] — call its
    /// `.dismiss()` at the same site.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.actions.clear();
        self.selected_index = 0;
    }
}

impl LspCodeActionsPopup {
    /// Reset state
    pub fn reset(&mut self) {
        self.visible = false;
        self.actions.clear();
        self.selected_index = 0;
        self.range = None;
    }
}

/// Per-editor inlay hints state.
///
/// Was `bevy_lsp::InlayHintState` (Resource).
#[derive(Component, Default)]
pub struct LspInlayHints {
    /// Cached inlay hints for current view
    pub hints: Vec<InlayHint>,
    /// The range for which hints are cached
    pub cached_range: Option<Range>,
    /// Whether hints need to be refreshed
    pub needs_refresh: bool,
}

impl LspInlayHints {
    /// Check if a range is covered by the cache
    pub fn is_range_cached(&self, range: &Range) -> bool {
        if let Some(cached) = &self.cached_range {
            cached.start.line <= range.start.line && cached.end.line >= range.end.line
        } else {
            false
        }
    }

    /// Invalidate the cache
    pub fn invalidate(&mut self) {
        self.hints.clear();
        self.cached_range = None;
        self.needs_refresh = true;
    }
}

/// A pending LSP request (position-based). `position` is already in the
/// negotiated wire encoding — convert with [`bevy_lsp::rope_char_to_lsp_position`]
/// at enqueue time.
#[derive(Clone, Debug)]
pub struct PendingLspRequest {
    pub uri: Url,
    pub position: Position,
}

/// Per-feature LSP debounce timers for popup-driving requests
/// (cursor-stops-then-fire pattern). Completion + hover use this
/// shared component; document highlights and code actions live on
/// their own popup state Components ([`LspDocumentHighlights`],
/// [`LspCodeActionsPopup`]) and run their own timers there.
#[derive(Component)]
pub struct LspDebounceTimers {
    /// Completion: armed by `listen_completion_requests` from
    /// `LspConfig::completion::delay_ms`.
    pub completion_timer: Timer,
    pub pending_completion: Option<PendingLspRequest>,

    /// Hover: armed by `listen_hover_requests` from
    /// `LspConfig::hover::delay_ms`.
    pub hover_timer: Timer,
    pub pending_hover: Option<PendingLspRequest>,
}

impl Default for LspDebounceTimers {
    fn default() -> Self {
        // Durations are placeholders — the request-arming code re-sets each
        // timer's duration from `LspConfig` before resetting it, so what
        // matters here is that the timer starts in a `finished()` state.
        let t = |secs: f32| {
            let mut timer = Timer::from_seconds(secs, TimerMode::Once);
            timer.tick(std::time::Duration::from_secs_f32(secs));
            timer
        };
        Self {
            completion_timer: t(0.1),
            pending_completion: None,
            hover_timer: t(0.3),
            pending_hover: None,
        }
    }
}

/// Per-editor `textDocument/didChange` batcher.
///
/// `listen_text_edit_events` pushes one [`TextDocumentContentChangeEvent`]
/// per editor edit and resets `timer`; `sync_lsp_document` flushes the
/// accumulated batch in a single notification when the timer expires.
/// Mirrors the debounced approach used by Zed / VS Code / Helix: typing
/// bursts collapse into one server reparse, but trailing edits still
/// reach the server within `LspConfig::did_change_delay_ms`.
///
/// `LspDocument` owns `uri` and `version`; this Component owns the
/// pending payload + cadence.
#[derive(Component)]
pub struct LspDidChangeBatcher {
    /// Pending incremental change events, in edit order.
    pub pending: Vec<TextDocumentContentChangeEvent>,
    /// When set, the next flush sends a full-document sync instead of
    /// the accumulated incremental batch. Set on first edit without a
    /// pre-edit rope snapshot, or when `LspConfig::full_document_sync`
    /// is on. Cleared after every flush.
    pub force_full_doc: bool,
    /// Debounce timer; reset on every queued edit, fires when typing pauses.
    pub timer: Timer,
}

impl Default for LspDidChangeBatcher {
    fn default() -> Self {
        // Duration overwritten on first edit from `LspConfig::did_change_delay_ms`.
        Self {
            pending: Vec::new(),
            force_full_doc: false,
            timer: Timer::from_seconds(0.15, TimerMode::Once),
        }
    }
}

/// Per-editor document highlight state (all occurrences of symbol under cursor).
///
/// Was `bevy_lsp::DocumentHighlightState` (Resource).
#[derive(Component, Default)]
pub struct LspDocumentHighlights {
    /// Current highlights
    pub highlights: Vec<DocumentHighlight>,
    /// The cursor position for which highlights were requested
    pub cursor_position: usize,
    /// Whether highlights are currently visible
    pub visible: bool,
    /// Timer for debouncing highlight requests
    pub debounce_timer: Option<Timer>,
    pub in_flight_position: Option<usize>,
}

impl LspDocumentHighlights {
    /// Reset state
    pub fn reset(&mut self) {
        self.highlights.clear();
        self.visible = false;
        self.debounce_timer = None;
    }

    /// Clear highlights without resetting timer
    pub fn clear_highlights(&mut self) {
        self.highlights.clear();
        self.visible = false;
    }
}

/// Per-editor rename popup state.
///
/// Was `bevy_lsp::RenameState` (Resource).
#[derive(Component, Default)]
pub struct LspRenamePopup {
    /// Whether rename dialog is visible
    pub visible: bool,
    /// The range being renamed
    pub range: Option<Range>,
    /// The original text being renamed
    pub original_text: String,
    /// The new name being typed
    pub new_name: String,
    /// Position where rename was initiated
    pub position: Option<Position>,
    /// Error message if rename failed
    pub error: Option<String>,
}

impl LspRenamePopup {
    /// Reset state
    pub fn reset(&mut self) {
        self.visible = false;
        self.range = None;
        self.original_text.clear();
        self.new_name.clear();
        self.position = None;
        self.error = None;
    }

    /// Start preparing rename at position. The "preparing" flag now
    /// lives implicitly on [`RenameLifecycle`] — non-zero `request_id`
    /// with `popup_entity == None` means a prepareRename is in flight.
    pub fn start_prepare(&mut self, position: Position) {
        self.reset();
        self.position = Some(position);
    }

    /// Handle prepare rename response
    pub fn on_prepare_response(&mut self, range: Range, placeholder: Option<String>) {
        self.range = Some(range);
        self.original_text = placeholder.clone().unwrap_or_default();
        self.new_name = placeholder.unwrap_or_default();
        self.visible = true;
    }

    /// Check if rename is ready to submit
    pub fn can_submit(&self) -> bool {
        self.visible && !self.new_name.is_empty() && self.new_name != self.original_text
    }
}

pub use super::lifecycle::{
    CodeActionsLifecycle, CodeActionsPopupBackref, CompletionLifecycle, CompletionPopupBackref,
    HoverLifecycle, HoverPopupBackref, PopupLifecycleData, PopupObserversAttached,
    RenameLifecycle, RenamePopupBackref, SignatureLifecycle, SignaturePopupBackref,
};
